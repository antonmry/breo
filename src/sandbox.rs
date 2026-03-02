use crate::config::{Backend, backend_name};
use std::io;
use std::process::Command;

pub(crate) fn build_command(backend: &Backend, model: Option<&str>) -> Command {
    match backend {
        Backend::Claude => {
            let mut cmd = Command::new("claude");
            cmd.arg("--dangerously-skip-permissions");
            cmd.arg("--print");
            if let Some(model) = model {
                cmd.arg("--model").arg(model);
            }
            cmd
        }
        Backend::Codex => {
            let mut cmd = Command::new("codex");
            cmd.arg("exec").arg("--full-auto");
            if let Some(model) = model {
                cmd.arg("--model").arg(model);
            }
            cmd
        }
        Backend::Gemini => {
            let mut cmd = Command::new("gemini");
            cmd.arg("--yolo");
            if let Some(model) = model {
                cmd.arg("--model").arg(model);
            }
            cmd
        }
    }
}

pub(crate) fn check_sandbox(name: &str) {
    match Command::new("limactl")
        .arg("list")
        .arg("--format={{.Name}}")
        .output()
    {
        Err(_) => {
            eprintln!(
                "Sandbox '{name}' requires Lima but 'limactl' was not found.\n\
                 Install Lima (https://lima-vm.io) or use --no-sandbox."
            );
            std::process::exit(1);
        }
        Ok(output) => {
            let vms = String::from_utf8_lossy(&output.stdout);
            if !vms.lines().any(|line| line.trim() == name) {
                eprintln!(
                    "Lima VM '{name}' not found.\n\
                     Available VMs: {}\n\
                     Create it with 'limactl start {name}' or use --no-sandbox.",
                    if vms.trim().is_empty() {
                        "(none)".to_string()
                    } else {
                        vms.lines().map(|l| l.trim()).collect::<Vec<_>>().join(", ")
                    }
                );
                std::process::exit(1);
            }
        }
    }
}

pub(crate) fn build_sandbox_command(
    sandbox_name: &str,
    backend: &Backend,
    model: Option<&str>,
) -> Command {
    let mut cmd = Command::new("limactl");
    cmd.arg("shell").arg(sandbox_name);

    match backend {
        Backend::Claude => {
            cmd.arg("claude")
                .arg("--dangerously-skip-permissions")
                .arg("--print");
            if let Some(m) = model {
                cmd.arg("--model").arg(m);
            }
        }
        Backend::Codex => {
            cmd.arg("codex").arg("exec").arg("--full-auto");
            if let Some(m) = model {
                cmd.arg("--model").arg(m);
            }
        }
        Backend::Gemini => {
            cmd.arg("gemini").arg("--yolo");
            if let Some(m) = model {
                cmd.arg("--model").arg(m);
            }
        }
    }
    cmd
}

pub(crate) fn execute_command_inner(
    cmd: Command,
    prompt: &str,
    sandboxed: bool,
    backend: &Backend,
    stream: bool,
    line_tx: Option<std::sync::mpsc::Sender<String>>,
) -> (String, String, bool) {
    let bin = if sandboxed {
        "limactl"
    } else {
        backend_name(backend)
    };
    let mut cmd = cmd;
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::inherit());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to run {bin}: {e}");
            std::process::exit(1);
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        use io::Write;
        let _ = stdin.write_all(prompt.as_bytes());
    }

    let mut stdout_buf = String::new();
    if let Some(pipe) = child.stdout.take() {
        let reader = io::BufReader::new(pipe);
        use io::BufRead;
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if stream {
                        println!("{l}");
                        use io::Write;
                        let _ = io::stdout().flush();
                    }
                    if let Some(ref tx) = line_tx {
                        let _ = tx.send(l.clone());
                    }
                    stdout_buf.push_str(&l);
                    stdout_buf.push('\n');
                }
                Err(e) => {
                    eprintln!("Error reading {bin} stdout: {e}");
                    break;
                }
            }
        }
    }

    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to wait for {bin}: {e}");
            std::process::exit(1);
        }
    };

    (stdout_buf, String::new(), status.success())
}

pub(crate) fn execute_command(
    cmd: Command,
    prompt: &str,
    sandboxed: bool,
    backend: &Backend,
) -> (String, String, bool) {
    execute_command_inner(cmd, prompt, sandboxed, backend, true, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_parts(cmd: Command) -> Vec<String> {
        let prog = cmd.get_program().to_string_lossy().to_string();
        let args = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        std::iter::once(prog).chain(args).collect()
    }

    #[test]
    fn build_command_claude_default() {
        let parts = command_parts(build_command(&Backend::Claude, None));
        assert_eq!(parts[0], "claude");
        assert!(parts.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(parts.contains(&"--print".to_string()));
    }

    #[test]
    fn build_command_claude_with_model() {
        let parts = command_parts(build_command(&Backend::Claude, Some("opus")));
        assert!(parts.windows(2).any(|w| w == ["--model", "opus"]));
    }

    #[test]
    fn build_command_codex_default() {
        let parts = command_parts(build_command(&Backend::Codex, None));
        assert_eq!(parts[0], "codex");
        assert!(parts.contains(&"exec".to_string()));
        assert!(parts.contains(&"--full-auto".to_string()));
    }

    #[test]
    fn build_command_gemini_default() {
        let parts = command_parts(build_command(&Backend::Gemini, None));
        assert_eq!(parts[0], "gemini");
        assert!(parts.contains(&"--yolo".to_string()));
    }

    #[test]
    fn build_sandbox_command_claude() {
        let parts = command_parts(build_sandbox_command("default", &Backend::Claude, None));
        assert_eq!(parts[..4], ["limactl", "shell", "default", "claude"]);
    }

    #[test]
    fn build_sandbox_command_codex_with_model() {
        let parts = command_parts(build_sandbox_command(
            "default",
            &Backend::Codex,
            Some("gpt-5"),
        ));
        assert_eq!(parts[..4], ["limactl", "shell", "default", "codex"]);
        assert!(parts.windows(2).any(|w| w == ["--model", "gpt-5"]));
    }

    #[test]
    fn build_command_gemini_with_model() {
        let parts = command_parts(build_command(&Backend::Gemini, Some("gemini-3-pro")));
        assert_eq!(parts[0], "gemini");
        assert!(parts.windows(2).any(|w| w == ["--model", "gemini-3-pro"]));
    }

    #[test]
    fn build_command_codex_with_model() {
        let parts = command_parts(build_command(&Backend::Codex, Some("gpt-5")));
        assert_eq!(parts[0], "codex");
        assert!(parts.windows(2).any(|w| w == ["--model", "gpt-5"]));
    }

    #[test]
    fn build_sandbox_command_gemini_default() {
        let parts = command_parts(build_sandbox_command("vm1", &Backend::Gemini, None));
        assert_eq!(parts[..4], ["limactl", "shell", "vm1", "gemini"]);
        assert!(parts.contains(&"--yolo".to_string()));
    }

    #[test]
    fn build_sandbox_command_gemini_with_model() {
        let parts = command_parts(build_sandbox_command(
            "vm1",
            &Backend::Gemini,
            Some("gemini-2.5-pro"),
        ));
        assert_eq!(parts[..4], ["limactl", "shell", "vm1", "gemini"]);
        assert!(parts.windows(2).any(|w| w == ["--model", "gemini-2.5-pro"]));
    }

    #[test]
    fn execute_command_inner_with_echo() {
        let mut cmd = Command::new("echo");
        cmd.arg("hello world");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.contains("hello world"));
    }

    #[test]
    fn execute_command_inner_streaming() {
        let mut cmd = Command::new("echo");
        cmd.arg("stream-test");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, true, None);
        assert!(success);
        assert!(stdout.contains("stream-test"));
    }

    #[test]
    fn execute_command_wrapper() {
        let mut cmd = Command::new("echo");
        cmd.arg("wrapper-test");
        let (stdout, _, success) = execute_command(cmd, "", false, &Backend::Claude);
        assert!(success);
        assert!(stdout.contains("wrapper-test"));
    }

    #[test]
    fn execute_command_inner_sandboxed_label() {
        // Test with a command that always succeeds
        let cmd = Command::new("true");
        let (_, _, success) = execute_command_inner(cmd, "", true, &Backend::Claude, false, None);
        assert!(success);
    }

    #[test]
    fn execute_command_inner_with_stdin_prompt() {
        // cat reads stdin and outputs it
        let cmd = Command::new("cat");
        let (stdout, _, success) =
            execute_command_inner(cmd, "from-stdin", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.contains("from-stdin"));
    }

    #[test]
    fn execute_command_inner_failing_command() {
        let cmd = Command::new("false");
        let (_, _, success) = execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(!success);
    }

    #[test]
    fn execute_command_inner_multiline_output() {
        let mut cmd = Command::new("printf");
        cmd.arg("line1\nline2\nline3");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.contains("line1"));
        assert!(stdout.contains("line2"));
        assert!(stdout.contains("line3"));
    }

    #[test]
    fn execute_command_inner_streaming_multiline() {
        let mut cmd = Command::new("printf");
        cmd.arg("a\nb\nc");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, true, None);
        assert!(success);
        assert!(stdout.contains("a"));
        assert!(stdout.contains("b"));
        assert!(stdout.contains("c"));
    }

    #[test]
    fn execute_command_inner_empty_output() {
        let cmd = Command::new("true");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.is_empty());
    }

    #[test]
    fn execute_command_inner_large_stdin() {
        let cmd = Command::new("cat");
        let large_input = "x".repeat(100_000);
        let (stdout, _, success) =
            execute_command_inner(cmd, &large_input, false, &Backend::Claude, false, None);
        assert!(success);
        assert_eq!(stdout.trim(), large_input);
    }

    #[test]
    fn build_command_all_backends_have_required_args() {
        for backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            let parts = command_parts(build_command(&backend, None));
            assert!(!parts.is_empty());
            // Each backend should have at least program + one arg
            assert!(parts.len() >= 2);
        }
    }

    #[test]
    fn build_sandbox_command_all_backends_start_with_limactl() {
        for backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            let parts = command_parts(build_sandbox_command("vm", &backend, None));
            assert_eq!(parts[0], "limactl");
            assert_eq!(parts[1], "shell");
            assert_eq!(parts[2], "vm");
        }
    }

    #[test]
    fn execute_command_delegates_to_inner_with_stream() {
        let mut cmd = Command::new("echo");
        cmd.arg("delegate-test");
        let (stdout, _, success) = execute_command(cmd, "", false, &Backend::Claude);
        assert!(success);
        assert!(stdout.contains("delegate-test"));
    }

    #[test]
    fn execute_command_inner_sandboxed_true_label() {
        // When sandboxed=true, internal label should be "limactl"
        // Just verify it doesn't crash
        let cmd = Command::new("echo");
        let (_, _, success) = execute_command_inner(cmd, "", true, &Backend::Claude, false, None);
        assert!(success);
    }

    #[test]
    fn build_command_codex_has_exec_full_auto() {
        let parts = command_parts(build_command(&Backend::Codex, None));
        assert!(parts.contains(&"exec".to_string()));
        assert!(parts.contains(&"--full-auto".to_string()));
    }

    #[test]
    fn build_command_gemini_has_yolo() {
        let parts = command_parts(build_command(&Backend::Gemini, None));
        assert!(parts.contains(&"--yolo".to_string()));
    }

    #[test]
    fn build_command_claude_has_print_and_skip_permissions() {
        let parts = command_parts(build_command(&Backend::Claude, None));
        assert!(parts.contains(&"--print".to_string()));
        assert!(parts.contains(&"--dangerously-skip-permissions".to_string()));
    }

    // --- build_sandbox_command with model for all backends ---

    #[test]
    fn build_sandbox_command_claude_with_model() {
        let parts = command_parts(build_sandbox_command(
            "default",
            &Backend::Claude,
            Some("opus"),
        ));
        assert_eq!(parts[0], "limactl");
        assert_eq!(parts[1], "shell");
        assert_eq!(parts[2], "default");
        assert_eq!(parts[3], "claude");
        assert!(parts.windows(2).any(|w| w == ["--model", "opus"]));
    }

    #[test]
    fn build_sandbox_command_codex_without_model() {
        let parts = command_parts(build_sandbox_command("vm2", &Backend::Codex, None));
        assert_eq!(parts[..4], ["limactl", "shell", "vm2", "codex"]);
        assert!(parts.contains(&"exec".to_string()));
        assert!(parts.contains(&"--full-auto".to_string()));
        assert!(!parts.contains(&"--model".to_string()));
    }

    #[test]
    fn build_sandbox_command_gemini_without_model() {
        let parts = command_parts(build_sandbox_command("vm3", &Backend::Gemini, None));
        assert_eq!(parts[..4], ["limactl", "shell", "vm3", "gemini"]);
        assert!(parts.contains(&"--yolo".to_string()));
        assert!(!parts.contains(&"--model".to_string()));
    }

    // --- more build_command edge case tests ---

    #[test]
    fn build_command_claude_no_model_no_model_flag() {
        let parts = command_parts(build_command(&Backend::Claude, None));
        assert!(!parts.contains(&"--model".to_string()));
    }

    #[test]
    fn build_command_codex_no_model_no_model_flag() {
        let parts = command_parts(build_command(&Backend::Codex, None));
        assert!(!parts.contains(&"--model".to_string()));
    }

    #[test]
    fn build_command_gemini_no_model_no_model_flag() {
        let parts = command_parts(build_command(&Backend::Gemini, None));
        assert!(!parts.contains(&"--model".to_string()));
    }

    // --- execute_command_inner edge cases ---

    #[test]
    fn execute_command_inner_empty_stdin() {
        let cmd = Command::new("cat");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.is_empty());
    }

    #[test]
    fn execute_command_inner_unicode_output() {
        let mut cmd = Command::new("printf");
        cmd.arg("hello 🌍 café");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.contains("hello"));
    }

    #[test]
    fn execute_command_inner_exit_code_nonzero() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("exit 42");
        let (_, _, success) = execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(!success);
    }

    #[test]
    fn execute_command_inner_stderr_inherited() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo err >&2; echo out");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.contains("out"));
    }

    // --- more build_sandbox_command parameterized tests ---

    #[test]
    fn build_sandbox_command_with_model_always_has_model_flag() {
        for backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            let parts = command_parts(build_sandbox_command("vm", &backend, Some("test-model")));
            assert!(
                parts.windows(2).any(|w| w == ["--model", "test-model"]),
                "missing --model for {:?}",
                backend
            );
        }
    }

    #[test]
    fn build_sandbox_command_without_model_never_has_model_flag() {
        for backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            let parts = command_parts(build_sandbox_command("vm", &backend, None));
            assert!(
                !parts.contains(&"--model".to_string()),
                "unexpected --model for {:?}",
                backend
            );
        }
    }

    #[test]
    fn build_command_with_model_always_has_model_flag() {
        for backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            let parts = command_parts(build_command(&backend, Some("my-model")));
            assert!(
                parts.windows(2).any(|w| w == ["--model", "my-model"]),
                "missing --model for {:?}",
                backend
            );
        }
    }

    #[test]
    fn execute_command_inner_line_tx_receives_lines() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut cmd = Command::new("printf");
        cmd.arg("alpha\nbeta\ngamma");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, Some(tx));
        assert!(success);
        assert!(stdout.contains("alpha"));
        let lines: Vec<String> = rx.try_iter().collect();
        assert_eq!(lines, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn execute_command_inner_line_tx_none_still_works() {
        let mut cmd = Command::new("echo");
        cmd.arg("no-tx");
        let (stdout, _, success) =
            execute_command_inner(cmd, "", false, &Backend::Claude, false, None);
        assert!(success);
        assert!(stdout.contains("no-tx"));
    }
}
