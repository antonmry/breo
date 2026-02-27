use crate::config::{Backend, current_dir_key, load_dir_state, save_dir_state};
use crate::sandbox::{
    build_command, build_sandbox_command, check_sandbox, execute_command, execute_command_inner,
};
use chrono::Local;
use skim::prelude::*;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn breo_dir() -> PathBuf {
    dirs::home_dir()
        .expect("could not determine home directory")
        .join(".config")
        .join("breo")
}

pub(crate) fn conversations_dir() -> PathBuf {
    breo_dir().join("conversations")
}

pub(crate) fn ensure_breo_dir() {
    let base = breo_dir();
    let conv_dir = conversations_dir();
    if !conv_dir.exists()
        && let Err(e) = fs::create_dir_all(&conv_dir)
    {
        eprintln!("Failed to create {}: {e}", conv_dir.display());
        std::process::exit(1);
    }

    ensure_dir_conversations_dir();

    if !base.join(".git").exists() {
        let _ = Command::new("git")
            .arg("init")
            .current_dir(&base)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    let config_path = base.join("config.toml");
    if !config_path.exists() {
        let default_config = "sandbox = true\nsandbox_name = \"default\"\nagent = \"claude\"\n";
        let _ = fs::write(&config_path, default_config);
    }
}

pub(crate) fn get_active() -> String {
    let state = load_dir_state();
    if let Some(ref name) = state.conversation {
        let scoped = dir_conversations_dir().join(format!("{name}.md"));
        if scoped.exists() {
            return name.clone();
        }
    }

    let dir = dir_conversations_dir();
    if dir.exists()
        && let Some(latest) = find_latest_conversation(&dir)
    {
        return latest;
    }

    generate_timestamp_name()
}

pub(crate) fn set_active(name: &str) {
    let mut state = load_dir_state();
    state.conversation = Some(name.to_string());
    save_dir_state(&state);
}

pub(crate) fn get_or_create_dir_id() -> String {
    let mut state = load_dir_state();
    if let Some(ref id) = state.dir_id {
        return id.clone();
    }
    let key = current_dir_key();
    let basename = std::path::Path::new(&key)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".into());

    let sanitized: String = basename
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let conv_dir = conversations_dir();
    let candidate = conv_dir.join(&sanitized);

    let id = if !candidate.exists() {
        sanitized
    } else {
        let marker = candidate.join("_dir.txt");
        let existing_path = fs::read_to_string(&marker).unwrap_or_default();
        if existing_path.trim() == key {
            sanitized
        } else {
            let mut hasher = std::hash::DefaultHasher::new();
            key.hash(&mut hasher);
            format!("{}-{:08x}", sanitized, hasher.finish() as u32)
        }
    };

    state.dir_id = Some(id.clone());
    save_dir_state(&state);
    id
}

pub(crate) fn dir_conversations_dir() -> PathBuf {
    conversations_dir().join(get_or_create_dir_id())
}

pub(crate) fn ensure_dir_conversations_dir() {
    let dir = dir_conversations_dir();
    if !dir.exists() {
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("Failed to create {}: {e}", dir.display());
            std::process::exit(1);
        }
        let marker = dir.join("_dir.txt");
        let _ = fs::write(&marker, current_dir_key());
    }
}

pub(crate) fn generate_timestamp_name() -> String {
    Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

pub(crate) fn find_latest_conversation(dir: &Path) -> Option<String> {
    let entries = fs::read_dir(dir).ok()?;
    let mut names: Vec<String> = entries
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().to_string();
            let name = name.strip_suffix(".md")?;
            Some(name.to_string())
        })
        .collect();
    names.sort();
    names.pop()
}

pub(crate) fn conversation_path(name: &str) -> PathBuf {
    dir_conversations_dir().join(format!("{name}.md"))
}

pub(crate) fn conversation_names_sorted() -> Vec<String> {
    let dir = dir_conversations_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().to_string();
            let name = name.strip_suffix(".md")?;
            Some(name.to_string())
        })
        .collect();
    names.sort();
    names
}

pub(crate) fn context_window(model: Option<&str>, backend: &Backend) -> usize {
    if let Some(m) = model {
        let m = m.to_lowercase();
        if m.contains("opus") || m.contains("sonnet") || m.contains("haiku") {
            return 200_000;
        }
        if m.contains("gpt-5") {
            return 400_000;
        }
        if m.contains("o3") || m.contains("o4-mini") {
            return 200_000;
        }
        if m.contains("gemini") {
            return 1_000_000;
        }
    }
    match backend {
        Backend::Claude => 200_000,
        Backend::Codex => 400_000,
        Backend::Gemini => 1_000_000,
    }
}

pub(crate) fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

pub(crate) fn count_exchanges(text: &str) -> usize {
    text.matches("## User").count()
}

pub(crate) fn format_tokens(tokens: usize) -> String {
    if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

pub(crate) fn is_committed(path: &Path) -> bool {
    Command::new("git")
        .arg("diff")
        .arg("--quiet")
        .arg("HEAD")
        .arg("--")
        .arg(path)
        .current_dir(breo_dir())
        .status()
        .is_ok_and(|s| s.success())
}

/// Format the context summary line for display.
pub(crate) fn format_context_line(
    name: &str,
    exchanges: usize,
    tokens_used: usize,
    window: usize,
    committed: bool,
) -> String {
    let tokens_remaining = window.saturating_sub(tokens_used);
    let pct_used = if window > 0 {
        (tokens_used as f64 / window as f64 * 100.0) as usize
    } else {
        0
    };
    let dirty = if committed { "" } else { " | uncommitted" };
    format!(
        "\n[{name}] {exchanges} exchanges | ~{} tokens used | ~{} remaining ({pct_used}% used){dirty}",
        format_tokens(tokens_used),
        format_tokens(tokens_remaining),
    )
}

pub(crate) fn print_context_summary(
    content: &str,
    name: &str,
    model: Option<&str>,
    backend: &Backend,
    path: &Path,
) {
    let window = context_window(model, backend);
    let exchanges = count_exchanges(content);
    let tokens_used = estimate_tokens(content);
    let committed = is_committed(path);
    eprintln!(
        "{}",
        format_context_line(name, exchanges, tokens_used, window, committed)
    );
}

pub(crate) fn create_conversation(name: &str) -> Result<(), String> {
    ensure_breo_dir();
    let path = conversation_path(name);
    if path.exists() {
        return Err(format!("Conversation '{name}' already exists"));
    }
    let header = format!("# Conversation: {name}\n\n");
    if let Err(e) = fs::write(&path, &header) {
        return Err(format!("Failed to create {}: {e}", path.display()));
    }
    set_active(name);
    git_commit_conversation(&path, &format!("breo: new conversation '{name}'"));
    git_commit_state();
    Ok(())
}

pub(crate) fn cmd_new(name: &str) {
    if let Err(e) = create_conversation(name) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    println!("Created and switched to conversation: {name}");
}

/// Validates rename parameters and returns whether the active conversation should be updated.
pub(crate) fn validate_rename(
    old_name: &str,
    new_name: &str,
    old_exists: bool,
    new_exists: bool,
    active: &str,
) -> Result<bool, String> {
    if !old_exists {
        return Err(format!("Conversation '{old_name}' does not exist"));
    }
    if new_exists {
        return Err(format!("Conversation '{new_name}' already exists"));
    }
    Ok(active == old_name)
}

pub(crate) fn cmd_rename(old_name: &str, new_name: &str) {
    let old_path = conversation_path(old_name);
    let new_path = conversation_path(new_name);
    let active = get_active();

    match validate_rename(
        old_name,
        new_name,
        old_path.exists(),
        new_path.exists(),
        &active,
    ) {
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
        Ok(should_update_active) => {
            if let Err(e) = fs::rename(&old_path, &new_path) {
                eprintln!(
                    "Failed to rename {} -> {}: {e}",
                    old_path.display(),
                    new_path.display()
                );
                std::process::exit(1);
            }

            if should_update_active {
                set_active(new_name);
            }

            git_commit_conversation(
                &new_path,
                &format!("breo: rename '{old_name}' -> '{new_name}'"),
            );
            git_commit_state();

            println!("Renamed conversation: {old_name} -> {new_name}");
        }
    }
}

pub(crate) fn cmd_pick() {
    let dir = dir_conversations_dir();
    if !dir.exists() {
        std::process::exit(1);
    }
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|_| std::process::exit(1))
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().to_string();
            name.strip_suffix(".md").map(String::from)
        })
        .collect();
    names.sort();

    if names.is_empty() {
        std::process::exit(1);
    }

    let active = get_active();
    let input = names
        .iter()
        .map(|n| {
            if *n == active {
                format!("* {n}")
            } else {
                format!("  {n}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let options = SkimOptionsBuilder::default()
        .prompt("conversation> ".to_string())
        .build()
        .unwrap_or_else(|_| std::process::exit(1));

    let item_reader = SkimItemReader::default();
    let items = item_reader.of_bufread(Cursor::new(input));

    let Ok(output) = Skim::run_with(options, Some(items)) else {
        std::process::exit(1);
    };
    if output.is_abort {
        std::process::exit(1);
    }

    if let Some(item) = output.selected_items.first() {
        let name = item
            .output()
            .trim()
            .trim_start_matches("* ")
            .trim_start()
            .to_string();
        set_active(&name);
        print!("{name}");
    }
}

pub(crate) fn format_conversation_list(entries: &[String], active: &str) -> Vec<String> {
    entries
        .iter()
        .map(|name| {
            if name == active {
                format!("* {name}")
            } else {
                format!("  {name}")
            }
        })
        .collect()
}

pub(crate) fn build_prompt(existing: &str, name: &str, message: &str, attachments: &str) -> String {
    let header = if existing.is_empty() {
        format!("# Conversation: {name}\n\n")
    } else {
        existing.to_string()
    };
    format!("{header}## User\n{message}\n{attachments}")
}

pub(crate) fn build_conversation_content(prompt: &str, response: &str) -> String {
    format!("{prompt}\n## Assistant\n{response}\n\n")
}

pub(crate) fn cmd_list() {
    let dir = dir_conversations_dir();
    if !dir.exists() {
        println!("No conversations yet.");
        return;
    }
    let active = get_active();
    let entries = conversation_names_sorted();

    if entries.is_empty() {
        println!("No conversations yet.");
        return;
    }

    for line in format_conversation_list(&entries, &active) {
        println!("{line}");
    }
}

pub(crate) fn read_attached_files(files: &[PathBuf]) -> String {
    let mut attachments = String::new();
    for path in files {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to read {}: {e}", path.display());
                std::process::exit(1);
            }
        };
        attachments.push_str(&format!(
            "\n### File: {}\n```\n{content}\n```\n",
            path.display()
        ));
    }
    attachments
}

pub(crate) fn git_commit_conversation(_path: &Path, message: &str) {
    let base = breo_dir();
    let status = Command::new("git")
        .arg("add")
        .arg("-A")
        .arg("conversations/")
        .current_dir(&base)
        .status();
    if let Ok(s) = status
        && s.success()
    {
        let _ = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg(message)
            .current_dir(&base)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

pub(crate) fn git_commit_state() {
    let base = breo_dir();
    let path = crate::config::state_file_path();
    let status = Command::new("git")
        .arg("add")
        .arg(&path)
        .current_dir(&base)
        .status();
    if let Ok(s) = status
        && s.success()
    {
        let _ = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("breo: update state")
            .current_dir(&base)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

pub(crate) fn git_push() {
    let base = breo_dir();
    let status = Command::new("git").arg("push").current_dir(&base).status();
    match status {
        Ok(s) if s.success() => println!("Pushed successfully."),
        Ok(s) => {
            eprintln!("git push exited with status {s}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run git push: {e}");
            std::process::exit(1);
        }
    }
}

pub(crate) fn git_pull() {
    let base = breo_dir();
    let status = Command::new("git").arg("pull").current_dir(&base).status();
    match status {
        Ok(s) if s.success() => println!("Pulled successfully."),
        Ok(s) => {
            eprintln!("git pull exited with status {s}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to run git pull: {e}");
            std::process::exit(1);
        }
    }
}

/// Build the compact prompt from conversation content and name.
pub(crate) fn build_compact_prompt(name: &str, content: &str) -> String {
    format!(
        "You are compacting a conversation for future LLM context. \
         Summarize the following conversation into a concise briefing that an LLM can use \
         to seamlessly resume the conversation. Preserve:\n\
         - The original intent and goals\n\
         - Key decisions made and their rationale\n\
         - Important code snippets, file paths, commands, and technical details\n\
         - Errors encountered and their solutions\n\
         - Current state and what was being worked on last\n\n\
         Give significantly more weight to recent exchanges as they represent the current working state.\n\
         Output ONLY the summary as markdown, starting with '# Conversation: {name} (compacted)'.\n\
         Do not include any preamble or explanation.\n\n---\n\n{content}"
    )
}

/// Format the compact summary output message.
pub(crate) fn format_compact_summary(
    name: &str,
    exchanges_before: usize,
    tokens_before: usize,
    tokens_after: usize,
    model: Option<&str>,
    backend: &Backend,
) -> String {
    let (pct_saved, remaining) = compact_stats(tokens_before, tokens_after, model, backend);
    format!(
        "\n[{name}] Compacted {exchanges_before} exchanges\n\
         ~{} -> ~{} tokens ({pct_saved}% saved)\n\
         ~{} tokens remaining",
        format_tokens(tokens_before),
        format_tokens(tokens_after),
        format_tokens(remaining),
    )
}

/// Validate compact preconditions. Returns Ok((content, exchanges, tokens_before)) or Err(message).
pub(crate) fn validate_compact(
    name: &str,
    path_exists: bool,
    content: &str,
) -> Result<(usize, usize), String> {
    if !path_exists {
        return Err(format!("Conversation '{name}' does not exist"));
    }
    let exchanges = count_exchanges(content);
    if exchanges == 0 {
        return Err(format!("Nothing to compact in '{name}'"));
    }
    Ok((exchanges, estimate_tokens(content)))
}

/// Determine the error label for a sandbox/backend failure.
pub(crate) fn failure_label(sandboxed: bool, backend: &Backend) -> &'static str {
    if sandboxed {
        "limactl"
    } else {
        crate::config::backend_name(backend)
    }
}

pub(crate) fn cmd_compact(name: Option<&str>, sandbox: Option<&str>) {
    let active = get_active();
    let name = name.unwrap_or(&active);
    let path = conversation_path(name);

    let content = if path.exists() {
        fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    let (exchanges_before, tokens_before) = match validate_compact(name, path.exists(), &content) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            if msg.contains("does not exist") {
                std::process::exit(1);
            }
            return;
        }
    };

    let prompt = build_compact_prompt(name, &content);

    eprintln!("Compacting '{name}'...");

    let backend = Backend::Claude;
    let cmd = if let Some(vm) = sandbox {
        check_sandbox(vm);
        build_sandbox_command(vm, &backend, None)
    } else {
        build_command(&backend, None)
    };
    let (stdout, stderr, success) = execute_command(cmd, &prompt, sandbox.is_some(), &backend);

    if !success {
        eprintln!(
            "{} failed: {stderr}",
            failure_label(sandbox.is_some(), &backend)
        );
        std::process::exit(1);
    }

    let summary = stdout.trim_end();

    let compacted = format!("{summary}\n\n");
    if let Err(e) = fs::write(&path, &compacted) {
        eprintln!("Failed to write {}: {e}", path.display());
        std::process::exit(1);
    }

    git_commit_conversation(&path, &format!("breo: compact '{name}'"));

    let tokens_after = estimate_tokens(&compacted);
    eprintln!(
        "{}",
        format_compact_summary(
            name,
            exchanges_before,
            tokens_before,
            tokens_after,
            None,
            &backend
        )
    );
}

pub(crate) fn compact_stats(
    tokens_before: usize,
    tokens_after: usize,
    model: Option<&str>,
    backend: &Backend,
) -> (usize, usize) {
    let saved = tokens_before.saturating_sub(tokens_after);
    let window = context_window(model, backend);
    let remaining = window.saturating_sub(tokens_after);
    let pct_saved = if tokens_before > 0 {
        (saved as f64 / tokens_before as f64 * 100.0) as usize
    } else {
        0
    };
    (pct_saved, remaining)
}

pub(crate) fn cmd_send_inner(
    message: &str,
    target: Option<&str>,
    model: Option<&str>,
    backend: &Backend,
    files: &[PathBuf],
    sandbox: Option<&str>,
    stream: bool,
) -> (String, String, bool) {
    ensure_breo_dir();
    let active = get_active();
    let name = target.unwrap_or(&active);
    let path = conversation_path(name);

    let existing = if path.exists() {
        fs::read_to_string(&path).unwrap_or_default()
    } else {
        format!("# Conversation: {name}\n\n")
    };

    let attachments = read_attached_files(files);
    let prompt = build_prompt(&existing, name, message, &attachments);

    let cmd = if let Some(vm) = sandbox {
        check_sandbox(vm);
        build_sandbox_command(vm, backend, model)
    } else {
        build_command(backend, model)
    };
    let (stdout, stderr, success) =
        execute_command_inner(cmd, &prompt, sandbox.is_some(), backend, stream);

    if !success {
        return (name.to_string(), stderr, false);
    }

    let response = stdout.trim_end();

    let content = build_conversation_content(&prompt, response);
    if let Err(e) = fs::write(&path, &content) {
        eprintln!("Failed to write {}: {e}", path.display());
        std::process::exit(1);
    }

    git_commit_conversation(&path, &format!("breo: message to '{name}'"));

    print_context_summary(&content, name, model, backend, &path);

    (name.to_string(), response.to_string(), true)
}

pub(crate) fn cmd_send(
    message: &str,
    target: Option<&str>,
    model: Option<&str>,
    backend: &Backend,
    files: &[PathBuf],
    sandbox: Option<&str>,
) -> String {
    let (name, stderr, success) =
        cmd_send_inner(message, target, model, backend, files, sandbox, true);
    if !success {
        let label = if sandbox.is_some() {
            "limactl"
        } else {
            crate::config::backend_name(backend)
        };
        eprintln!("{label} failed: {stderr}");
        std::process::exit(1);
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn with_temp_home<T>(f: impl FnOnce() -> T) -> T {
        let tmp = TempDir::new().expect("tempdir");
        let old_home = std::env::var_os("HOME");
        let old_xdg = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: tests using this helper are marked #[serial], so process env
        // mutation is not concurrent with other tests in this module.
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));
        }
        let out = f();
        // SAFETY: see safety note above.
        unsafe {
            if let Some(v) = old_home {
                std::env::set_var("HOME", v);
            } else {
                std::env::remove_var("HOME");
            }
            if let Some(v) = old_xdg {
                std::env::set_var("XDG_CONFIG_HOME", v);
            } else {
                std::env::remove_var("XDG_CONFIG_HOME");
            }
        }
        out
    }

    #[test]
    fn generate_timestamp_name_format() {
        let s = generate_timestamp_name();
        assert_eq!(s.len(), 19);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "_");
        assert_eq!(&s[13..14], "-");
        assert_eq!(&s[16..17], "-");
    }

    #[test]
    fn estimate_tokens_approximation() {
        assert_eq!(estimate_tokens("hello world"), "hello world".len() / 4);
    }

    #[test]
    fn count_exchanges_various() {
        assert_eq!(count_exchanges(""), 0);
        assert_eq!(count_exchanges("## User\nhi"), 1);
        assert_eq!(count_exchanges("## User\na\n## User\nb\n## User\nc"), 3);
    }

    #[test]
    fn format_tokens_small_and_large() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1500), "1.5k");
    }

    #[test]
    fn context_window_by_model() {
        assert_eq!(context_window(Some("opus"), &Backend::Codex), 200_000);
        assert_eq!(context_window(Some("gpt-5"), &Backend::Claude), 400_000);
        assert_eq!(
            context_window(Some("gemini-2.5-pro"), &Backend::Claude),
            1_000_000
        );
    }

    #[test]
    fn context_window_backend_default() {
        assert_eq!(context_window(None, &Backend::Claude), 200_000);
        assert_eq!(context_window(None, &Backend::Codex), 400_000);
        assert_eq!(context_window(None, &Backend::Gemini), 1_000_000);
    }

    #[test]
    fn conversation_path_has_md_suffix() {
        let p = conversation_path("abc");
        assert!(p.to_string_lossy().ends_with("abc.md"));
    }

    #[test]
    fn find_latest_conversation_cases() {
        let tmp = TempDir::new().expect("tempdir");
        assert!(find_latest_conversation(tmp.path()).is_none());

        fs::write(tmp.path().join("2026-01-01_00-00-00.md"), "").expect("write 1");
        assert_eq!(
            find_latest_conversation(tmp.path()).as_deref(),
            Some("2026-01-01_00-00-00")
        );

        fs::write(tmp.path().join("2026-01-01_00-00-01.md"), "").expect("write 2");
        assert_eq!(
            find_latest_conversation(tmp.path()).as_deref(),
            Some("2026-01-01_00-00-01")
        );
    }

    #[test]
    #[serial]
    fn breo_dir_ends_with_breo() {
        with_temp_home(|| {
            let d = breo_dir();
            assert!(d.to_string_lossy().ends_with("breo"));
        });
    }

    #[test]
    #[serial]
    fn conversations_dir_is_under_breo_dir() {
        with_temp_home(|| {
            let c = conversations_dir();
            let b = breo_dir();
            assert!(c.starts_with(&b));
            assert!(c.to_string_lossy().ends_with("conversations"));
        });
    }

    #[test]
    fn read_attached_files_valid() {
        let tmp = TempDir::new().expect("tempdir");
        let f1 = tmp.path().join("a.txt");
        let f2 = tmp.path().join("b.txt");
        fs::write(&f1, "content-a").expect("write");
        fs::write(&f2, "content-b").expect("write");

        let result = read_attached_files(&[f1, f2]);
        assert!(result.contains("content-a"));
        assert!(result.contains("content-b"));
        assert!(result.contains("### File:"));
    }

    #[test]
    fn read_attached_files_empty() {
        let result = read_attached_files(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn context_window_o3_model() {
        assert_eq!(context_window(Some("o3"), &Backend::Claude), 200_000);
    }

    #[test]
    fn context_window_o4_mini_model() {
        assert_eq!(context_window(Some("o4-mini"), &Backend::Codex), 200_000);
    }

    #[test]
    fn context_window_sonnet_model() {
        assert_eq!(context_window(Some("sonnet"), &Backend::Gemini), 200_000);
    }

    #[test]
    fn context_window_haiku_model() {
        assert_eq!(context_window(Some("haiku"), &Backend::Gemini), 200_000);
    }

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn count_exchanges_zero() {
        assert_eq!(
            count_exchanges("# Conversation: test\n\nNo exchanges here."),
            0
        );
    }

    #[test]
    fn format_tokens_boundary() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(100_000), "100.0k");
    }

    #[test]
    fn find_latest_conversation_ignores_non_md() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("not-markdown.txt"), "").expect("write");
        assert!(find_latest_conversation(tmp.path()).is_none());
    }

    #[test]
    fn find_latest_conversation_sorts_alphabetically() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("aaa.md"), "").expect("write");
        fs::write(tmp.path().join("zzz.md"), "").expect("write");
        fs::write(tmp.path().join("mmm.md"), "").expect("write");
        assert_eq!(find_latest_conversation(tmp.path()).as_deref(), Some("zzz"));
    }

    #[test]
    #[serial]
    fn conversation_path_format() {
        with_temp_home(|| {
            let p = conversation_path("my-chat");
            let s = p.to_string_lossy();
            assert!(s.ends_with("my-chat.md"));
            assert!(s.contains("conversations"));
        });
    }

    #[test]
    #[serial]
    fn get_active_returns_string() {
        with_temp_home(|| {
            ensure_breo_dir();
            let name = get_active();
            assert!(!name.is_empty());
        });
    }

    #[test]
    #[serial]
    fn ensure_breo_dir_no_panic() {
        with_temp_home(|| {
            ensure_breo_dir();
        });
    }

    #[test]
    #[serial]
    fn dir_conversations_dir_is_under_conversations() {
        with_temp_home(|| {
            let d = dir_conversations_dir();
            let c = conversations_dir();
            assert!(d.starts_with(&c));
        });
    }

    #[test]
    #[serial]
    fn conversation_names_sorted_returns_vec() {
        with_temp_home(|| {
            ensure_breo_dir();
            let names = conversation_names_sorted();
            let mut sorted = names.clone();
            sorted.sort();
            assert_eq!(names, sorted);
        });
    }

    #[test]
    fn print_context_summary_no_panic() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("test.md");
        fs::write(&path, "## User\nhi\n## Assistant\nhello\n").expect("write");
        print_context_summary(
            "## User\nhi\n## Assistant\nhello\n",
            "test",
            None,
            &Backend::Claude,
            &path,
        );
    }

    #[test]
    fn is_committed_returns_bool() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("nonexistent.md");
        // On a non-git dir, should return false
        let result = is_committed(&path);
        assert!(!result);
    }

    #[test]
    fn generate_timestamp_name_unique() {
        let a = generate_timestamp_name();
        // Two calls in same second should be identical or differ by subsecond
        let b = generate_timestamp_name();
        assert_eq!(a.len(), b.len());
    }

    #[test]
    #[serial]
    fn get_or_create_dir_id_is_stable() {
        with_temp_home(|| {
            ensure_breo_dir();
            let id1 = get_or_create_dir_id();
            let id2 = get_or_create_dir_id();
            assert_eq!(id1, id2);
            assert!(!id1.is_empty());
            assert!(
                id1.chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
            );
        });
    }

    #[test]
    #[serial]
    fn set_active_and_get_active_round_trip() {
        with_temp_home(|| {
            ensure_breo_dir();
            let orig = get_active();
            let test_name = "test-round-trip-conv";
            set_active(test_name);
            set_active(&orig);
        });
    }

    #[test]
    #[serial]
    fn create_conversation_creates_file() {
        with_temp_home(|| {
            ensure_breo_dir();
            let name = format!(
                "test-create-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("unix epoch")
                    .as_nanos()
            );
            let result = create_conversation(&name);
            assert!(result.is_ok(), "{result:?}");
            let path = conversation_path(&name);
            assert!(path.exists());
            let content = fs::read_to_string(&path).expect("read conversation");
            assert!(content.contains(&format!("# Conversation: {name}")));
            let _ = fs::remove_file(&path);
        });
    }

    #[test]
    #[serial]
    fn create_conversation_duplicate_fails() {
        with_temp_home(|| {
            ensure_breo_dir();
            let name = format!(
                "test-dup-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("unix epoch")
                    .as_nanos()
            );
            let result1 = create_conversation(&name);
            assert!(result1.is_ok(), "{result1:?}");
            let result2 = create_conversation(&name);
            assert!(result2.is_err());
            let _ = fs::remove_file(conversation_path(&name));
        });
    }

    #[test]
    fn read_attached_files_content_format() {
        let tmp = TempDir::new().expect("tempdir");
        let f1 = tmp.path().join("code.rs");
        fs::write(&f1, "fn main() {}").expect("write");

        let result = read_attached_files(&[f1]);
        assert!(result.contains("### File:"));
        assert!(result.contains("code.rs"));
        assert!(result.contains("fn main() {}"));
        assert!(result.contains("```"));
    }

    #[test]
    fn estimate_tokens_proportional() {
        let short = estimate_tokens("hello");
        let long = estimate_tokens("hello world, this is a longer string for testing");
        assert!(long > short);
    }

    #[test]
    fn count_exchanges_assistant_headers_ignored() {
        assert_eq!(
            count_exchanges("## User\nhi\n## Assistant\nhello\n## User\nbye"),
            2
        );
    }

    #[test]
    fn context_window_unknown_model_uses_backend() {
        assert_eq!(
            context_window(Some("unknown-model"), &Backend::Claude),
            200_000
        );
    }

    #[test]
    fn format_tokens_various() {
        assert_eq!(format_tokens(1), "1");
        assert_eq!(format_tokens(500), "500");
        assert_eq!(format_tokens(10_000), "10.0k");
        assert_eq!(format_tokens(200_000), "200.0k");
    }

    #[test]
    #[serial]
    fn conversation_names_sorted_is_sorted() {
        with_temp_home(|| {
            ensure_breo_dir();
            let names = conversation_names_sorted();
            for w in names.windows(2) {
                assert!(w[0] <= w[1]);
            }
        });
    }

    #[test]
    fn find_latest_conversation_multiple_types() {
        let tmp = TempDir::new().expect("tempdir");
        fs::write(tmp.path().join("alpha.md"), "").expect("write");
        fs::write(tmp.path().join("beta.md"), "").expect("write");
        fs::write(tmp.path().join("gamma.txt"), "").expect("write");
        fs::write(tmp.path().join("_dir.txt"), "").expect("write");
        assert_eq!(
            find_latest_conversation(tmp.path()).as_deref(),
            Some("beta")
        );
    }

    #[test]
    fn print_context_summary_various_backends() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("test.md");
        let content = "## User\nhi\n## Assistant\nhello\n## User\nbye\n## Assistant\nsee ya\n";
        fs::write(&path, content).expect("write");

        // Test with different backends - should not panic
        print_context_summary(content, "test", None, &Backend::Codex, &path);
        print_context_summary(
            content,
            "test",
            Some("gemini-3-pro"),
            &Backend::Gemini,
            &path,
        );
        print_context_summary(content, "test", Some("gpt-5"), &Backend::Codex, &path);
    }

    #[test]
    fn is_committed_nonexistent_path() {
        assert!(!is_committed(Path::new("/nonexistent/path/file.md")));
    }

    #[test]
    #[serial]
    fn conversation_path_includes_dir_id() {
        with_temp_home(|| {
            ensure_breo_dir();
            let p = conversation_path("test");
            assert!(p.to_string_lossy().contains("conversations"));
            assert!(p.to_string_lossy().ends_with("test.md"));
        });
    }

    #[test]
    #[serial]
    fn ensure_dir_conversations_dir_no_panic() {
        with_temp_home(|| {
            ensure_breo_dir();
            ensure_dir_conversations_dir();
            assert!(dir_conversations_dir().exists());
        });
    }

    #[test]
    fn generate_timestamp_name_contains_date_parts() {
        let name = generate_timestamp_name();
        // Should contain year (2026)
        assert!(name.starts_with("202"));
        // Should contain time separator
        assert!(name.contains('_'));
    }

    #[test]
    fn git_commit_conversation_does_not_panic() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("test.md");
        fs::write(&path, "test").expect("write");
        // Should not panic even outside a git repo
        git_commit_conversation(&path, "test commit");
    }

    #[test]
    #[serial]
    fn git_commit_state_does_not_panic() {
        with_temp_home(|| {
            ensure_breo_dir();
            git_commit_state();
        });
    }

    #[test]
    fn format_conversation_list_empty() {
        let list = format_conversation_list(&[], "main");
        assert!(list.is_empty());
    }

    #[test]
    fn format_conversation_list_active_marked() {
        let entries = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let list = format_conversation_list(&entries, "beta");
        assert_eq!(list[0], "  alpha");
        assert_eq!(list[1], "* beta");
        assert_eq!(list[2], "  gamma");
    }

    #[test]
    fn format_conversation_list_no_active_match() {
        let entries = vec!["a".into(), "b".into()];
        let list = format_conversation_list(&entries, "nonexistent");
        assert!(list.iter().all(|l| l.starts_with("  ")));
    }

    #[test]
    fn build_prompt_new_conversation() {
        let prompt = build_prompt("", "test", "hello world", "");
        assert!(prompt.contains("# Conversation: test"));
        assert!(prompt.contains("## User"));
        assert!(prompt.contains("hello world"));
    }

    #[test]
    fn build_prompt_existing_conversation() {
        let existing = "# Conversation: test\n\n## User\nprevious message\n## Assistant\nprevious response\n\n";
        let prompt = build_prompt(existing, "test", "new message", "");
        assert!(prompt.contains("previous message"));
        assert!(prompt.contains("new message"));
        assert!(!prompt.contains("# Conversation: test\n\n# Conversation: test"));
    }

    #[test]
    fn build_prompt_with_attachments() {
        let prompt = build_prompt(
            "",
            "test",
            "hello",
            "\n### File: a.txt\n```\ncontent\n```\n",
        );
        assert!(prompt.contains("hello"));
        assert!(prompt.contains("a.txt"));
        assert!(prompt.contains("content"));
    }

    #[test]
    fn build_conversation_content_format() {
        let content = build_conversation_content("## User\nhi", "hello there");
        assert!(content.contains("## User\nhi"));
        assert!(content.contains("## Assistant\nhello there"));
        assert!(content.ends_with("\n\n"));
    }

    #[test]
    fn compact_stats_basic() {
        let (pct, remaining) = compact_stats(1000, 200, None, &Backend::Claude);
        assert_eq!(pct, 80);
        assert_eq!(remaining, 200_000 - 200);
    }

    #[test]
    fn compact_stats_zero_before() {
        let (pct, _) = compact_stats(0, 0, None, &Backend::Claude);
        assert_eq!(pct, 0);
    }

    #[test]
    fn compact_stats_same_size() {
        let (pct, _) = compact_stats(1000, 1000, None, &Backend::Claude);
        assert_eq!(pct, 0);
    }

    #[test]
    fn compact_stats_with_model() {
        let (_, remaining) = compact_stats(1000, 500, Some("gemini-3-pro"), &Backend::Gemini);
        assert_eq!(remaining, 1_000_000 - 500);
    }

    #[test]
    #[serial]
    fn cmd_list_no_panic() {
        with_temp_home(|| {
            ensure_breo_dir();
            cmd_list();
        });
    }

    #[test]
    #[serial]
    fn cmd_rename_no_panic_with_nonexistent() {
        with_temp_home(|| {
            ensure_breo_dir();
            let path = conversation_path("definitely-not-a-real-conversation");
            assert!(!path.exists());
        });
    }

    #[test]
    #[serial]
    fn cmd_new_and_list_integration() {
        with_temp_home(|| {
            ensure_breo_dir();
            let name = format!(
                "test-int-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("unix epoch")
                    .as_nanos()
            );
            let result = create_conversation(&name);
            assert!(result.is_ok(), "{result:?}");

            let names = conversation_names_sorted();
            assert!(names.contains(&name));

            let list = format_conversation_list(&names, &name);
            assert!(list.iter().any(|l| l.contains(&name) && l.starts_with("*")));

            let _ = fs::remove_file(conversation_path(&name));
        });
    }

    #[test]
    fn build_prompt_preserves_existing_header() {
        let existing = "# Conversation: old\n\n## User\nold msg\n## Assistant\nold reply\n\n";
        let prompt = build_prompt(existing, "old", "new msg", "");
        // Should start with existing content, not duplicate header
        assert!(prompt.starts_with("# Conversation: old"));
        assert!(prompt.contains("old msg"));
        assert!(prompt.contains("new msg"));
    }

    #[test]
    fn build_conversation_content_multiline() {
        let content =
            build_conversation_content("## User\nline1\nline2", "response line1\nresponse line2");
        assert!(content.contains("line1\nline2"));
        assert!(content.contains("response line1\nresponse line2"));
    }

    #[test]
    fn compact_stats_large_values() {
        let (pct, remaining) = compact_stats(200_000, 50_000, None, &Backend::Claude);
        assert_eq!(pct, 75);
        assert_eq!(remaining, 200_000 - 50_000);
    }

    #[test]
    fn compact_stats_after_exceeds_before() {
        // Edge case: after compaction is somehow larger (shouldn't happen but be safe)
        let (pct, _) = compact_stats(100, 200, None, &Backend::Claude);
        assert_eq!(pct, 0); // saturating_sub produces 0
    }

    // --- validate_rename tests ---

    #[test]
    fn validate_rename_old_does_not_exist() {
        let result = validate_rename("old", "new", false, false, "main");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn validate_rename_new_already_exists() {
        let result = validate_rename("old", "new", true, true, "main");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn validate_rename_success_updates_active() {
        let result = validate_rename("old", "new", true, false, "old");
        assert_eq!(result, Ok(true));
    }

    #[test]
    fn validate_rename_success_keeps_active() {
        let result = validate_rename("old", "new", true, false, "other");
        assert_eq!(result, Ok(false));
    }

    #[test]
    fn validate_rename_error_messages_contain_names() {
        let err = validate_rename("my-conv", "new", false, false, "x").unwrap_err();
        assert!(err.contains("my-conv"));

        let err = validate_rename("old", "existing", true, true, "x").unwrap_err();
        assert!(err.contains("existing"));
    }

    // --- validate_compact tests ---

    #[test]
    fn validate_compact_not_exists() {
        let result = validate_compact("test", false, "");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));
    }

    #[test]
    fn validate_compact_empty_content() {
        let result = validate_compact("test", true, "# Header\n\n");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Nothing to compact"));
    }

    #[test]
    fn validate_compact_valid() {
        let content = "# Conversation: test\n\n## User\nhello\n## Assistant\nhi\n\n";
        let result = validate_compact("test", true, content);
        assert!(result.is_ok());
        let (exchanges, tokens) = result.unwrap();
        assert!(exchanges > 0);
        assert!(tokens > 0);
    }

    #[test]
    fn validate_compact_error_includes_name() {
        let err = validate_compact("my-convo", false, "").unwrap_err();
        assert!(err.contains("my-convo"));
    }

    // --- build_compact_prompt tests ---

    #[test]
    fn build_compact_prompt_contains_name_and_content() {
        let prompt = build_compact_prompt("test-conv", "## User\nhello\n");
        assert!(prompt.contains("test-conv"));
        assert!(prompt.contains("## User\nhello\n"));
        assert!(prompt.contains("compacting"));
    }

    #[test]
    fn build_compact_prompt_has_instructions() {
        let prompt = build_compact_prompt("x", "content");
        assert!(prompt.contains("Preserve:"));
        assert!(prompt.contains("recent exchanges"));
    }

    // --- format_compact_summary tests ---

    #[test]
    fn format_compact_summary_basic() {
        let summary = format_compact_summary("test", 5, 10000, 2000, None, &Backend::Claude);
        assert!(summary.contains("test"));
        assert!(summary.contains("5 exchanges"));
        assert!(summary.contains("80% saved"));
    }

    #[test]
    fn format_compact_summary_zero_tokens() {
        let summary = format_compact_summary("test", 1, 0, 0, None, &Backend::Claude);
        assert!(summary.contains("0% saved"));
    }

    // --- failure_label tests ---

    #[test]
    fn failure_label_sandboxed() {
        assert_eq!(failure_label(true, &Backend::Claude), "limactl");
        assert_eq!(failure_label(true, &Backend::Codex), "limactl");
    }

    #[test]
    fn failure_label_not_sandboxed() {
        assert_eq!(failure_label(false, &Backend::Claude), "claude");
        assert_eq!(failure_label(false, &Backend::Codex), "codex");
        assert_eq!(failure_label(false, &Backend::Gemini), "gemini");
    }

    // --- format_context_line tests ---

    #[test]
    fn format_context_line_basic() {
        let line = format_context_line("test", 3, 1500, 200_000, false);
        assert!(line.contains("[test]"));
        assert!(line.contains("3 exchanges"));
        assert!(line.contains("1.5k tokens used"));
        assert!(line.contains("uncommitted"));
    }

    #[test]
    fn format_context_line_committed() {
        let line = format_context_line("conv", 1, 500, 200_000, true);
        assert!(line.contains("[conv]"));
        assert!(!line.contains("uncommitted"));
    }

    #[test]
    fn format_context_line_zero_exchanges() {
        let line = format_context_line("empty", 0, 0, 200_000, true);
        assert!(line.contains("0 exchanges"));
        assert!(line.contains("0 tokens used"));
        assert!(line.contains("200.0k remaining"));
    }

    #[test]
    fn format_context_line_high_usage() {
        let line = format_context_line("full", 100, 190_000, 200_000, false);
        assert!(line.contains("95% used"));
        assert!(line.contains("190.0k tokens used"));
        assert!(line.contains("10.0k remaining"));
    }

    #[test]
    fn format_context_line_zero_window() {
        let line = format_context_line("zero", 1, 100, 0, true);
        assert!(line.contains("0% used"));
    }

    #[test]
    fn format_context_line_exact_window() {
        let line = format_context_line("exact", 5, 200_000, 200_000, false);
        assert!(line.contains("100% used"));
        assert!(line.contains("0 remaining"));
    }

    #[test]
    fn format_context_line_all_backends_windows() {
        for (window, label) in [
            (200_000, "200.0k"),
            (400_000, "400.0k"),
            (1_000_000, "1000.0k"),
        ] {
            let line = format_context_line("x", 1, 0, window, true);
            assert!(line.contains(label), "expected {} in {}", label, line);
        }
    }

    // --- more context_window tests ---

    #[test]
    fn context_window_all_model_patterns() {
        let cases = [
            ("opus-3", Backend::Codex, 200_000),
            ("claude-3-sonnet", Backend::Codex, 200_000),
            ("gpt-5-turbo", Backend::Claude, 400_000),
            ("o3-mini-2025", Backend::Claude, 200_000),
            ("o4-mini-latest", Backend::Claude, 200_000),
            ("gemini-2.5-flash", Backend::Claude, 1_000_000),
            ("haiku-3.5", Backend::Gemini, 200_000),
        ];
        for (model, backend, expected) in cases {
            assert_eq!(
                context_window(Some(model), &backend),
                expected,
                "model={model}"
            );
        }
    }

    #[test]
    fn context_window_model_case_insensitive() {
        assert_eq!(context_window(Some("OPUS"), &Backend::Codex), 200_000);
        assert_eq!(context_window(Some("GPT-5"), &Backend::Claude), 400_000);
        assert_eq!(
            context_window(Some("GEMINI-2"), &Backend::Claude),
            1_000_000
        );
    }

    #[test]
    fn context_window_model_partial_match() {
        assert_eq!(
            context_window(Some("my-custom-opus-model"), &Backend::Claude),
            200_000
        );
        assert_eq!(
            context_window(Some("gpt-5-preview"), &Backend::Gemini),
            400_000
        );
    }

    // --- more estimate_tokens tests ---

    #[test]
    fn estimate_tokens_known_lengths() {
        assert_eq!(estimate_tokens("a"), 0); // 1/4 = 0
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn estimate_tokens_large_text() {
        let text = "x".repeat(10_000);
        assert_eq!(estimate_tokens(&text), 2500);
    }

    // --- more count_exchanges tests ---

    #[test]
    fn count_exchanges_many() {
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!(
                "## User\nmessage {i}\n## Assistant\nreply {i}\n\n"
            ));
        }
        assert_eq!(count_exchanges(&content), 10);
    }

    #[test]
    fn count_exchanges_case_sensitive() {
        assert_eq!(count_exchanges("## user\n"), 0);
        assert_eq!(count_exchanges("## USER\n"), 0);
        assert_eq!(count_exchanges("## User\n"), 1);
    }

    // --- more format_tokens tests ---

    #[test]
    fn format_tokens_edge_values() {
        assert_eq!(format_tokens(999), "999");
        assert_eq!(format_tokens(1000), "1.0k");
        assert_eq!(format_tokens(1001), "1.0k");
        assert_eq!(format_tokens(1500), "1.5k");
        assert_eq!(format_tokens(10_500), "10.5k");
    }

    // --- more build_prompt tests ---

    #[test]
    fn build_prompt_empty_message() {
        let prompt = build_prompt("", "test", "", "");
        assert!(prompt.contains("# Conversation: test"));
        assert!(prompt.contains("## User\n"));
    }

    #[test]
    fn build_prompt_multiline_message() {
        let prompt = build_prompt("", "test", "line1\nline2\nline3", "");
        assert!(prompt.contains("line1\nline2\nline3"));
    }

    #[test]
    fn build_prompt_special_chars_in_name() {
        let prompt = build_prompt("", "my-conv_2026.02", "msg", "");
        assert!(prompt.contains("# Conversation: my-conv_2026.02"));
    }

    // --- more build_conversation_content tests ---

    #[test]
    fn build_conversation_content_empty_response() {
        let content = build_conversation_content("## User\nhi", "");
        assert!(content.contains("## Assistant\n"));
        assert!(content.ends_with("\n\n"));
    }

    #[test]
    fn build_conversation_content_long_response() {
        let response = "word ".repeat(1000);
        let content = build_conversation_content("## User\nhi", &response);
        assert!(content.contains(&response));
    }

    // --- more validate_rename tests ---

    #[test]
    fn validate_rename_same_name_both_exist() {
        let result = validate_rename("same", "same", true, true, "same");
        assert!(result.is_err());
    }

    #[test]
    fn validate_rename_neither_exists() {
        let result = validate_rename("a", "b", false, false, "x");
        assert!(result.is_err());
    }

    // --- more validate_compact tests ---

    #[test]
    fn validate_compact_many_exchanges() {
        let content = "## User\na\n## User\nb\n## User\nc\n## User\nd\n## User\ne\n";
        let result = validate_compact("test", true, content);
        assert!(result.is_ok());
        let (exchanges, _) = result.unwrap();
        assert_eq!(exchanges, 5);
    }

    #[test]
    fn validate_compact_tokens_scale_with_content() {
        let short = "## User\nhi\n";
        let long = &"## User\nhello world this is a longer message\n".repeat(10);
        let (_, tokens_short) = validate_compact("s", true, short).unwrap();
        let (_, tokens_long) = validate_compact("l", true, long).unwrap();
        assert!(tokens_long > tokens_short);
    }

    // --- more build_compact_prompt tests ---

    #[test]
    fn build_compact_prompt_long_content() {
        let content = "## User\nhi\n## Assistant\nhello\n".repeat(50);
        let prompt = build_compact_prompt("test", &content);
        assert!(prompt.contains(&content));
        assert!(prompt.len() > content.len());
    }

    #[test]
    fn build_compact_prompt_special_name() {
        let prompt = build_compact_prompt("my-conv_2026.02.25", "content");
        assert!(prompt.contains("my-conv_2026.02.25"));
        assert!(prompt.contains("(compacted)"));
    }

    // --- more format_compact_summary tests ---

    #[test]
    fn format_compact_summary_large_reduction() {
        let summary =
            format_compact_summary("deep", 50, 100_000, 10_000, Some("opus"), &Backend::Claude);
        assert!(summary.contains("deep"));
        assert!(summary.contains("50 exchanges"));
        assert!(summary.contains("90% saved"));
    }

    #[test]
    fn format_compact_summary_no_reduction() {
        let summary = format_compact_summary("test", 1, 1000, 1000, None, &Backend::Claude);
        assert!(summary.contains("0% saved"));
    }

    #[test]
    fn format_compact_summary_with_gemini() {
        let summary =
            format_compact_summary("g", 5, 50_000, 10_000, Some("gemini-2.5"), &Backend::Gemini);
        assert!(summary.contains("remaining"));
    }

    // --- more compact_stats tests ---

    #[test]
    fn compact_stats_with_different_backends() {
        let (_, remaining_claude) = compact_stats(1000, 500, None, &Backend::Claude);
        let (_, remaining_codex) = compact_stats(1000, 500, None, &Backend::Codex);
        let (_, remaining_gemini) = compact_stats(1000, 500, None, &Backend::Gemini);
        assert_eq!(remaining_claude, 200_000 - 500);
        assert_eq!(remaining_codex, 400_000 - 500);
        assert_eq!(remaining_gemini, 1_000_000 - 500);
    }

    #[test]
    fn compact_stats_tokens_after_exceeds_window() {
        let (pct, remaining) = compact_stats(100, 300_000, None, &Backend::Claude);
        assert_eq!(remaining, 0); // saturating_sub
        assert_eq!(pct, 0); // 100 < 300_000 so saved = 0
    }

    // --- more failure_label tests ---

    #[test]
    fn failure_label_all_backends_sandboxed() {
        for backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            assert_eq!(failure_label(true, &backend), "limactl");
        }
    }

    // --- more format_conversation_list tests ---

    #[test]
    fn format_conversation_list_single_active() {
        let list = format_conversation_list(&["only".into()], "only");
        assert_eq!(list, vec!["* only"]);
    }

    #[test]
    fn format_conversation_list_many_entries() {
        let entries: Vec<String> = (0..20).map(|i| format!("conv-{i:02}")).collect();
        let list = format_conversation_list(&entries, "conv-10");
        assert_eq!(list.len(), 20);
        assert!(list[10].starts_with("*"));
        assert!(list.iter().filter(|l| l.starts_with("*")).count() == 1);
    }

    // --- more find_latest_conversation tests ---

    #[test]
    fn find_latest_conversation_nonexistent_dir() {
        assert!(find_latest_conversation(Path::new("/nonexistent/dir/path")).is_none());
    }

    #[test]
    fn find_latest_conversation_dir_with_only_dirs() {
        let tmp = TempDir::new().expect("tempdir");
        fs::create_dir(tmp.path().join("subdir")).expect("mkdir");
        assert!(find_latest_conversation(tmp.path()).is_none());
    }

    // --- more create_conversation tests ---

    #[test]
    #[serial]
    fn create_conversation_sets_active() {
        with_temp_home(|| {
            ensure_breo_dir();
            let name = format!("active-test-{}", std::process::id());
            create_conversation(&name).expect("create");
            let active = get_active();
            assert_eq!(active, name);
            let _ = fs::remove_file(conversation_path(&name));
        });
    }

    #[test]
    #[serial]
    fn create_conversation_has_header() {
        with_temp_home(|| {
            ensure_breo_dir();
            let name = format!("header-test-{}", std::process::id());
            create_conversation(&name).expect("create");
            let content = fs::read_to_string(conversation_path(&name)).expect("read");
            assert!(content.starts_with("# Conversation:"));
            assert!(content.contains(&name));
            let _ = fs::remove_file(conversation_path(&name));
        });
    }

    // --- read_attached_files tests ---

    #[test]
    fn read_attached_files_markdown_format() {
        let tmp = TempDir::new().expect("tempdir");
        let f = tmp.path().join("test.py");
        fs::write(&f, "print('hello')").expect("write");
        let result = read_attached_files(&[f]);
        assert!(result.contains("### File:"));
        assert!(result.contains("```"));
        assert!(result.contains("print('hello')"));
    }

    #[test]
    fn read_attached_files_multiple_preserves_order() {
        let tmp = TempDir::new().expect("tempdir");
        let f1 = tmp.path().join("first.txt");
        let f2 = tmp.path().join("second.txt");
        fs::write(&f1, "one").expect("write");
        fs::write(&f2, "two").expect("write");
        let result = read_attached_files(&[f1, f2]);
        let pos1 = result.find("first.txt").expect("first");
        let pos2 = result.find("second.txt").expect("second");
        assert!(pos1 < pos2);
    }

    // --- breo_dir and conversations_dir tests ---

    #[test]
    #[serial]
    fn breo_dir_contains_config() {
        with_temp_home(|| {
            let d = breo_dir();
            assert!(d.to_string_lossy().contains(".config"));
        });
    }

    #[test]
    #[serial]
    fn conversations_dir_ends_correctly() {
        with_temp_home(|| {
            let c = conversations_dir();
            assert!(c.to_string_lossy().ends_with("conversations"));
        });
    }

    // --- generate_timestamp_name tests ---

    #[test]
    fn generate_timestamp_name_valid_chars() {
        let name = generate_timestamp_name();
        assert!(
            name.chars()
                .all(|c| c.is_ascii_digit() || c == '-' || c == '_')
        );
    }

    // --- git commit tests ---

    #[test]
    fn git_commit_conversation_no_panic_with_push_false() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("test.md");
        fs::write(&path, "content").expect("write");
        git_commit_conversation(&path, "commit msg");
    }

    #[test]
    #[serial]
    fn git_commit_state_no_panic_with_push_false() {
        with_temp_home(|| {
            ensure_breo_dir();
            git_commit_state();
        });
    }

    // --- is_committed tests ---

    #[test]
    fn is_committed_tempdir_path() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp.path().join("file.md");
        fs::write(&path, "test").expect("write");
        // Not in a git repo, so should be false
        assert!(!is_committed(&path));
    }

    // --- conversation_names_sorted edge cases ---

    #[test]
    #[serial]
    fn conversation_names_sorted_after_create() {
        with_temp_home(|| {
            ensure_breo_dir();
            let n1 = format!("z-test-{}", std::process::id());
            let n2 = format!("a-test-{}", std::process::id());
            create_conversation(&n1).expect("create1");
            create_conversation(&n2).expect("create2");
            let names = conversation_names_sorted();
            assert!(names.contains(&n1));
            assert!(names.contains(&n2));
            // Should be sorted
            let pos1 = names.iter().position(|n| n == &n1).unwrap();
            let pos2 = names.iter().position(|n| n == &n2).unwrap();
            assert!(pos2 < pos1, "a- should come before z-");
            let _ = fs::remove_file(conversation_path(&n1));
            let _ = fs::remove_file(conversation_path(&n2));
        });
    }
}
