use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::engine::{
    ArgValueCandidates, ArgValueCompleter, CompletionCandidate, PathCompleter,
};
use clap_complete::env::CompleteEnv;
use serde::{Deserialize, Serialize};
use skim::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Cursor, IsTerminal, Read as _};
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize)]
#[serde(default)]
struct Config {
    sandbox: bool,
    sandbox_name: String,
    push: bool,
    agent: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sandbox: true,
            sandbox_name: "default".into(),
            push: true,
            agent: "claude".into(),
        }
    }
}

fn load_config() -> Config {
    let path = breo_dir().join("config.toml");
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct DirState {
    conversation: Option<String>,
    agent: Option<String>,
    sandbox: Option<String>,
    dir_id: Option<String>,
}

fn state_file_path() -> PathBuf {
    breo_dir().join("state.toml")
}

fn current_dir_key() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn load_all_state() -> HashMap<String, DirState> {
    let path = state_file_path();
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn save_all_state(map: &HashMap<String, DirState>) {
    let path = state_file_path();
    if let Ok(contents) = toml::to_string(map) {
        let _ = fs::write(&path, contents);
    }
}

fn load_dir_state() -> DirState {
    let key = current_dir_key();
    load_all_state().remove(&key).unwrap_or_default()
}

fn save_dir_state(state: &DirState) {
    let key = current_dir_key();
    let mut map = load_all_state();
    map.insert(key, state.clone());
    save_all_state(&map);
}

fn list_models() -> Vec<CompletionCandidate> {
    vec![
        // Claude
        CompletionCandidate::new("opus").help(Some("Claude Opus 4.6 (200k)".into())),
        CompletionCandidate::new("sonnet").help(Some("Claude Sonnet 4.5 (200k)".into())),
        CompletionCandidate::new("haiku").help(Some("Claude Haiku 4.5 (200k)".into())),
        // OpenAI
        CompletionCandidate::new("gpt-5").help(Some("GPT-5 (400k)".into())),
        CompletionCandidate::new("gpt-5-mini").help(Some("GPT-5 mini (400k)".into())),
        CompletionCandidate::new("o3").help(Some("o3 (200k)".into())),
        CompletionCandidate::new("o4-mini").help(Some("o4-mini (200k)".into())),
        // Gemini
        CompletionCandidate::new("gemini-2.5-pro").help(Some("Gemini 2.5 Pro (1M)".into())),
        CompletionCandidate::new("gemini-2.5-flash").help(Some("Gemini 2.5 Flash (1M)".into())),
    ]
}

fn list_conversations() -> Vec<CompletionCandidate> {
    let dir = dir_conversations_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().to_string();
            let name = name.strip_suffix(".md")?;
            Some(CompletionCandidate::new(name.to_string()))
        })
        .collect()
}

#[derive(Clone, ValueEnum)]
enum Backend {
    Claude,
    Codex,
    Gemini,
}

#[derive(Parser)]
#[command(
    name = "breo",
    about = "Chat with an LLM, keeping conversation in a markdown file"
)]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    /// The message to send
    message: Option<String>,

    /// Send to a specific conversation without switching the active one
    #[arg(short, long, add = ArgValueCandidates::new(list_conversations))]
    conversation: Option<String>,

    /// Model to use (e.g. sonnet, opus, o3, gpt-5, or a full model ID)
    #[arg(short, long, add = ArgValueCandidates::new(list_models))]
    model: Option<String>,

    /// Agent to use
    #[arg(short, long, value_enum)]
    agent: Option<Backend>,

    /// Files to attach to the prompt
    #[arg(short, long, num_args = 1.., add = ArgValueCompleter::new(PathCompleter::file()))]
    files: Vec<PathBuf>,

    /// Lima instance name for sandbox
    #[arg(short, long)]
    sandbox: Option<String>,

    /// Disable sandbox mode
    #[arg(long)]
    no_sandbox: bool,

    /// Disable auto-push after commit
    #[arg(long)]
    no_push: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new conversation and switch to it
    New { name: String },
    /// List all conversations
    List,
    /// Fuzzy-pick a conversation (for shell integration)
    Pick,
    /// Print shell setup for fuzzy TAB completion
    Setup {
        /// Shell type
        #[arg(value_enum)]
        shell: ShellType,
    },
    /// Show active conversation, agent, and sandbox for the current directory
    Status,
    /// Compact a conversation by summarizing it to save context
    Compact {
        /// Conversation to compact (defaults to active)
        #[arg(add = ArgValueCandidates::new(list_conversations))]
        name: Option<String>,
    },
    /// Run an implement/validate loop until the validator approves
    Loop {
        /// Path to the plan file (instructions for the implementer)
        plan: PathBuf,

        /// Path to the harness file (instructions for the validator)
        harness: PathBuf,

        /// Agent to use for the implementer
        #[arg(short, long, value_enum)]
        agent: Option<Backend>,

        /// Agent for the validator (defaults to same as --agent)
        #[arg(long, value_enum)]
        review_agent: Option<Backend>,

        /// Model for the validator (defaults to same as --model)
        #[arg(long, add = ArgValueCandidates::new(list_models))]
        review_model: Option<String>,

        /// Send to a specific conversation
        #[arg(short, long, add = ArgValueCandidates::new(list_conversations))]
        conversation: Option<String>,

        /// Files to attach to the implementer prompt
        #[arg(short, long, num_args = 1.., add = ArgValueCompleter::new(PathCompleter::file()))]
        files: Vec<PathBuf>,

        /// Lima instance name for sandbox
        #[arg(short, long)]
        sandbox: Option<String>,

        /// Disable sandbox mode
        #[arg(long)]
        no_sandbox: bool,
    },
}

#[derive(Clone, ValueEnum)]
enum ShellType {
    Bash,
    Zsh,
    Fish,
}

fn breo_dir() -> PathBuf {
    dirs::home_dir()
        .expect("could not determine home directory")
        .join(".config")
        .join("breo")
}

fn conversations_dir() -> PathBuf {
    breo_dir().join("conversations")
}

fn ensure_breo_dir() {
    let base = breo_dir();
    let conv_dir = conversations_dir();
    if !conv_dir.exists()
        && let Err(e) = fs::create_dir_all(&conv_dir)
    {
        eprintln!("Failed to create {}: {e}", conv_dir.display());
        std::process::exit(1);
    }

    ensure_dir_conversations_dir();

    // git init if .git doesn't exist
    if !base.join(".git").exists() {
        let _ = Command::new("git")
            .arg("init")
            .current_dir(&base)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    // Create default config.toml if missing
    let config_path = base.join("config.toml");
    if !config_path.exists() {
        let default_config =
            "sandbox = true\nsandbox_name = \"default\"\npush = true\nagent = \"claude\"\n";
        let _ = fs::write(&config_path, default_config);
    }
}

fn get_active() -> String {
    let state = load_dir_state();

    // 1. If explicitly set in state, use it (with lazy migration)
    if let Some(ref name) = state.conversation {
        let scoped = dir_conversations_dir().join(format!("{name}.md"));
        if scoped.exists() {
            return name.clone();
        }
        // Lazy migration: check old flat location
        let flat = conversations_dir().join(format!("{name}.md"));
        if flat.exists() {
            ensure_dir_conversations_dir();
            let _ = fs::copy(&flat, &scoped);
            return name.clone();
        }
        // Name set but file doesn't exist anywhere — fall through
    }

    // 2. Resume the latest conversation in this dir's subfolder
    let dir = dir_conversations_dir();
    if dir.exists()
        && let Some(latest) = find_latest_conversation(&dir)
    {
        return latest;
    }

    // 3. Auto-create a timestamped name (file created lazily by cmd_send)
    generate_timestamp_name()
}

fn set_active(name: &str) {
    let mut state = load_dir_state();
    state.conversation = Some(name.to_string());
    save_dir_state(&state);
}

fn get_or_create_dir_id() -> String {
    let mut state = load_dir_state();
    if let Some(ref id) = state.dir_id {
        return id.clone();
    }
    let key = current_dir_key();
    let basename = std::path::Path::new(&key)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".into());

    // Sanitize: keep alphanumeric, dash, underscore, dot
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
        // Check if existing dir points to the same path
        let marker = candidate.join("_dir.txt");
        let existing_path = fs::read_to_string(&marker).unwrap_or_default();
        if existing_path.trim() == key {
            sanitized
        } else {
            // Collision: append short hash
            let mut hasher = std::hash::DefaultHasher::new();
            key.hash(&mut hasher);
            format!("{}-{:08x}", sanitized, hasher.finish() as u32)
        }
    };

    state.dir_id = Some(id.clone());
    save_dir_state(&state);
    id
}

fn dir_conversations_dir() -> PathBuf {
    conversations_dir().join(get_or_create_dir_id())
}

fn ensure_dir_conversations_dir() {
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

fn generate_timestamp_name() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

fn find_latest_conversation(dir: &std::path::Path) -> Option<String> {
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

fn conversation_path(name: &str) -> PathBuf {
    dir_conversations_dir().join(format!("{name}.md"))
}

fn context_window(model: Option<&str>, backend: &Backend) -> usize {
    if let Some(m) = model {
        let m = m.to_lowercase();
        // Claude models
        if m.contains("opus") || m.contains("sonnet") || m.contains("haiku") {
            return 200_000;
        }
        // OpenAI models
        if m.contains("gpt-5") {
            return 400_000;
        }
        if m.contains("o3") || m.contains("o4-mini") {
            return 200_000;
        }
        // Gemini models
        if m.contains("gemini") {
            return 1_000_000;
        }
    }
    // Default per backend
    match backend {
        Backend::Claude => 200_000,   // claude-opus-4-6
        Backend::Codex => 400_000,    // gpt-5
        Backend::Gemini => 1_000_000, // gemini-2.5-pro
    }
}

fn estimate_tokens(text: &str) -> usize {
    // ~4 chars per token is a reasonable approximation for English text
    text.len() / 4
}

fn count_exchanges(text: &str) -> usize {
    text.matches("## User").count()
}

fn format_tokens(tokens: usize) -> String {
    if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn is_committed(path: &std::path::Path) -> bool {
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

fn print_context_summary(
    content: &str,
    name: &str,
    model: Option<&str>,
    backend: &Backend,
    path: &std::path::Path,
) {
    let window = context_window(model, backend);
    let exchanges = count_exchanges(content);
    let tokens_used = estimate_tokens(content);
    let tokens_remaining = window.saturating_sub(tokens_used);
    let pct_used = (tokens_used as f64 / window as f64 * 100.0) as usize;

    let dirty = if is_committed(path) {
        ""
    } else {
        " | uncommitted"
    };

    eprintln!(
        "\n[{name}] {exchanges} exchanges | ~{} tokens used | ~{} remaining ({pct_used}% used){dirty}",
        format_tokens(tokens_used),
        format_tokens(tokens_remaining),
    );
}

fn cmd_new(name: &str, push: bool) {
    ensure_breo_dir();
    let path = conversation_path(name);
    if path.exists() {
        eprintln!("Conversation '{name}' already exists");
        std::process::exit(1);
    }
    let header = format!("# Conversation: {name}\n\n");
    if let Err(e) = fs::write(&path, &header) {
        eprintln!("Failed to create {}: {e}", path.display());
        std::process::exit(1);
    }
    set_active(name);
    git_commit_state(push);
    println!("Created and switched to conversation: {name}");
}

fn cmd_pick() {
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
        .unwrap();

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
        print!("{name}");
    }
}

fn cmd_list() {
    let dir = dir_conversations_dir();
    if !dir.exists() {
        println!("No conversations yet.");
        return;
    }
    let active = get_active();
    let mut entries: Vec<String> = fs::read_dir(&dir)
        .unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", dir.display());
            std::process::exit(1);
        })
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            name.strip_suffix(".md").map(String::from)
        })
        .collect();
    entries.sort();

    if entries.is_empty() {
        println!("No conversations yet.");
        return;
    }

    for name in &entries {
        if *name == active {
            println!("* {name}");
        } else {
            println!("  {name}");
        }
    }
}

fn cmd_status() {
    let active = get_active();
    let state = load_dir_state();
    let agent = state.agent.as_deref().unwrap_or("(not set)");
    let sandbox = state.sandbox.as_deref().unwrap_or("(not set)");
    println!("directory:    {}", current_dir_key());
    println!("config:       {}", breo_dir().display());
    println!("conversations:{}", dir_conversations_dir().display());
    println!("conversation: {active}");
    println!("agent:        {agent}");
    println!("sandbox:      {sandbox}");
}

fn cmd_setup(shell: &ShellType) {
    let script = match shell {
        ShellType::Bash => {
            r#"# breo bash completion with fuzzy pick
# 1. Source clap completions (defines _clap_complete_breo)
source <(COMPLETE=bash breo)

# 2. Override with our skim-powered wrapper
_breo_with_pick() {
    local prev="${COMP_WORDS[COMP_CWORD-1]}"

    if [[ "$prev" == "-c" || "$prev" == "--conversation" ]] || \
       [[ "${COMP_WORDS[1]}" == "compact" && $COMP_CWORD -eq 2 ]]; then
        local pick
        pick=$(breo pick </dev/tty 2>/dev/tty)
        if [[ -n "$pick" ]]; then
            COMPREPLY=("${pick} ")
        fi
        return
    fi

    _clap_complete_breo "$@"
}
complete -o nospace -o bashdefault -o nosort -F _breo_with_pick breo"#
        }
        ShellType::Zsh => {
            r#"# breo zsh completion with fuzzy pick
# 1. Source clap completions (defines _clap_dynamic_completer_breo)
source <(COMPLETE=zsh breo)

# 2. Override with our skim-powered wrapper
_breo_with_pick() {
    if [[ "${words[-2]}" == "-c" || "${words[-2]}" == "--conversation" ]] || \
       [[ "${words[2]}" == "compact" && $CURRENT -eq 3 ]]; then
        local pick
        pick=$(breo pick </dev/tty 2>/dev/tty)
        if [[ -n "$pick" ]]; then
            compadd -S ' ' -- "$pick"
        fi
        return
    fi
    _clap_dynamic_completer_breo "$@"
}
compdef _breo_with_pick breo"#
        }
        ShellType::Fish => {
            r#"# breo fish completion with fuzzy pick
source (COMPLETE=fish breo | psub)

function __breo_pick_conversation
    breo pick </dev/tty 2>/dev/tty
end

complete -c breo -l conversation -s c -x -a '(__breo_pick_conversation)'
complete -c breo -n '__fish_seen_subcommand_from compact' -x -a '(__breo_pick_conversation)'"#
        }
    };
    println!("{script}");
}

fn build_command(backend: &Backend, model: Option<&str>) -> Command {
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

fn check_sandbox(name: &str) {
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

fn build_sandbox_command(sandbox_name: &str, backend: &Backend, model: Option<&str>) -> Command {
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

fn execute_command_inner(
    cmd: Command,
    prompt: &str,
    sandboxed: bool,
    backend: &Backend,
    stream: bool,
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

    // Write prompt to stdin, then close it
    if let Some(mut stdin) = child.stdin.take() {
        use io::Write;
        let _ = stdin.write_all(prompt.as_bytes());
        // stdin is dropped here, closing the pipe
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

fn execute_command(
    cmd: Command,
    prompt: &str,
    sandboxed: bool,
    backend: &Backend,
) -> (String, String, bool) {
    execute_command_inner(cmd, prompt, sandboxed, backend, true)
}

fn execute_command_quiet(
    cmd: Command,
    prompt: &str,
    sandboxed: bool,
    backend: &Backend,
) -> (String, String, bool) {
    execute_command_inner(cmd, prompt, sandboxed, backend, false)
}

fn backend_name(backend: &Backend) -> &'static str {
    match backend {
        Backend::Claude => "claude",
        Backend::Codex => "codex",
        Backend::Gemini => "gemini",
    }
}

fn read_attached_files(files: &[PathBuf]) -> String {
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

fn git_commit_conversation(_path: &std::path::Path, message: &str, push: bool) {
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
        let committed = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg(message)
            .current_dir(&base)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());

        if push && committed {
            let _ = Command::new("git")
                .arg("push")
                .current_dir(&base)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
}

fn git_commit_state(push: bool) {
    let base = breo_dir();
    let path = state_file_path();
    let status = Command::new("git")
        .arg("add")
        .arg(&path)
        .current_dir(&base)
        .status();
    if let Ok(s) = status
        && s.success()
    {
        let committed = Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("breo: update state")
            .current_dir(&base)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());

        if push && committed {
            let _ = Command::new("git")
                .arg("push")
                .current_dir(&base)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }
}

fn cmd_compact(name: Option<&str>, sandbox: Option<&str>, push: bool) {
    let active = get_active();
    let name = name.unwrap_or(&active);
    let path = conversation_path(name);

    if !path.exists() {
        eprintln!("Conversation '{name}' does not exist");
        std::process::exit(1);
    }

    let content = fs::read_to_string(&path).unwrap_or_default();
    let tokens_before = estimate_tokens(&content);
    let exchanges_before = count_exchanges(&content);

    if exchanges_before == 0 {
        eprintln!("Nothing to compact in '{name}'");
        return;
    }

    let prompt = format!(
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
    );

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
        let label = if sandbox.is_some() {
            "limactl"
        } else {
            backend_name(&backend)
        };
        eprintln!("{label} failed: {stderr}");
        std::process::exit(1);
    }

    let summary = stdout.trim_end();

    let compacted = format!("{summary}\n\n");
    if let Err(e) = fs::write(&path, &compacted) {
        eprintln!("Failed to write {}: {e}", path.display());
        std::process::exit(1);
    }

    git_commit_conversation(&path, &format!("breo: compact '{name}'"), push);

    let tokens_after = estimate_tokens(&compacted);
    let saved = tokens_before.saturating_sub(tokens_after);
    let window = context_window(None, &backend);
    let remaining = window.saturating_sub(tokens_after);
    let pct_saved = if tokens_before > 0 {
        (saved as f64 / tokens_before as f64 * 100.0) as usize
    } else {
        0
    };

    eprintln!(
        "\n[{name}] Compacted {exchanges_before} exchanges\n\
         ~{} -> ~{} tokens ({pct_saved}% saved)\n\
         ~{} tokens remaining",
        format_tokens(tokens_before),
        format_tokens(tokens_after),
        format_tokens(remaining),
    );
}

enum ReviewVerdict {
    Success,
    Retry(String),
}

fn parse_review(response: &str) -> ReviewVerdict {
    let upper = response.to_uppercase();
    if upper.contains("VERDICT: SUCCESS") {
        return ReviewVerdict::Success;
    }
    if upper.contains("VERDICT: RETRY") {
        // Extract feedback after FEEDBACK: (case-insensitive search)
        if let Some(pos) = upper.find("FEEDBACK:") {
            let feedback = response[pos + "FEEDBACK:".len()..].trim().to_string();
            return ReviewVerdict::Retry(feedback);
        }
        return ReviewVerdict::Retry(response.to_string());
    }
    // Fallback: treat as retry with full response
    ReviewVerdict::Retry(response.to_string())
}

fn truncate_display(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.len() > max {
        format!("{}...", &first_line[..max])
    } else {
        first_line.to_string()
    }
}

fn cmd_send(
    message: &str,
    target: Option<&str>,
    model: Option<&str>,
    backend: &Backend,
    files: &[PathBuf],
    sandbox: Option<&str>,
    push: bool,
) -> String {
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
    let prompt = format!("{existing}## User\n{message}\n{attachments}");

    let cmd = if let Some(vm) = sandbox {
        check_sandbox(vm);
        build_sandbox_command(vm, backend, model)
    } else {
        build_command(backend, model)
    };
    let (stdout, stderr, success) = execute_command(cmd, &prompt, sandbox.is_some(), backend);

    if !success {
        let label = if sandbox.is_some() {
            "limactl"
        } else {
            backend_name(backend)
        };
        eprintln!("{label} failed: {stderr}");
        std::process::exit(1);
    }

    let response = stdout.trim_end();

    let content = format!("{prompt}\n## Assistant\n{response}\n\n");
    if let Err(e) = fs::write(&path, &content) {
        eprintln!("Failed to write {}: {e}", path.display());
        std::process::exit(1);
    }

    git_commit_conversation(&path, &format!("breo: message to '{name}'"), push);

    print_context_summary(&content, name, model, backend, &path);

    name.to_string()
}

#[allow(clippy::too_many_arguments)]
fn cmd_loop(
    plan_path: &std::path::Path,
    harness_path: &std::path::Path,
    target: Option<&str>,
    model: Option<&str>,
    backend: &Backend,
    review_model: Option<&str>,
    review_backend: &Backend,
    files: &[PathBuf],
    sandbox: Option<&str>,
    push: bool,
) -> String {
    // Validate that plan and harness files are readable
    if let Err(e) = fs::metadata(plan_path) {
        eprintln!("Failed to read plan file {}: {e}", plan_path.display());
        std::process::exit(1);
    }
    if let Err(e) = fs::metadata(harness_path) {
        eprintln!(
            "Failed to read harness file {}: {e}",
            harness_path.display()
        );
        std::process::exit(1);
    }

    // Initialize RESULT.md in the working directory
    let result_path = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("RESULT.md");
    let result_initial = "# Result\n\n## Progress\n";
    if let Err(e) = fs::write(&result_path, result_initial) {
        eprintln!("Failed to create RESULT.md: {e}");
        std::process::exit(1);
    }

    eprintln!(
        "[loop] Plan: {} | Harness: {}",
        plan_path.display(),
        harness_path.display()
    );
    eprintln!("[loop] Result: RESULT.md");
    eprintln!(
        "[loop] Implementer: {} | Validator: {}",
        backend_name(backend),
        backend_name(review_backend)
    );
    eprintln!("[loop] Press Ctrl-C to stop at any time\n");

    // Build file references for extra attached files
    let file_refs = if files.is_empty() {
        String::new()
    } else {
        let paths: Vec<_> = files
            .iter()
            .map(|f| format!("  - {}", f.display()))
            .collect();
        format!("\nAlso read these reference files:\n{}\n", paths.join("\n"))
    };

    let result_instructions = "\n\nAfter completing your work, update RESULT.md with:\n\
         - A summary of changes made under a \"### Attempt N\" heading\n\
         - Files modified and why\n\
         - Any issues encountered and how they were resolved\n\
         - Lessons learned";

    // Attempt 1: send a short message referencing files (agent reads them from disk)
    eprintln!("[loop] === Attempt 1 ===");
    let first_message = format!(
        "Read the implementation plan from {} and follow the instructions.\n\
         {file_refs}{result_instructions}",
        plan_path.display()
    );
    let name = cmd_send(&first_message, target, model, backend, &[], sandbox, push);

    let mut iteration = 1;
    loop {
        eprintln!("\n[loop] Reviewing attempt {iteration}...");

        // Build and execute review via cmd_send to the reviewer
        let review_message = format!(
            "You are a validator reviewing an implementation attempt.\n\n\
             Read the acceptance criteria from {}.\n\
             Read RESULT.md for the implementation progress.\n\n\
             Review the implementation against the criteria.\n\
             After your review, update RESULT.md by appending under the current attempt:\n\
             - Your verdict (SUCCESS or RETRY)\n\
             - Specific feedback on what was done well and what needs fixing\n\
             - Concrete instructions for the next attempt (if RETRY)\n\n\
             Then respond with:\n\
             - VERDICT: SUCCESS (if all criteria met)\n\
             - VERDICT: RETRY + FEEDBACK: ... (if not)\n\n\
             Only return SUCCESS if the harness criteria are completely satisfied.",
            harness_path.display()
        );

        let cmd = if let Some(vm) = sandbox {
            build_sandbox_command(vm, review_backend, review_model)
        } else {
            build_command(review_backend, review_model)
        };
        let (stdout, stderr, success) =
            execute_command_quiet(cmd, &review_message, sandbox.is_some(), review_backend);

        if !success {
            let label = if sandbox.is_some() {
                "limactl"
            } else {
                backend_name(review_backend)
            };
            eprintln!("{label} failed during review: {stderr}");
            eprintln!("[loop] Stopping due to review error. Conversation: {name}");
            return name;
        }

        let response = stdout.trim();
        match parse_review(response) {
            ReviewVerdict::Success => {
                // Append final status to RESULT.md
                let final_status = format!(
                    "\n## Final Status\nCompleted successfully after {iteration} attempt(s).\n"
                );
                if let Ok(mut f) = fs::OpenOptions::new().append(true).open(&result_path) {
                    use io::Write;
                    let _ = f.write_all(final_status.as_bytes());
                }
                eprintln!("[loop] === SUCCESS after {} attempt(s) ===", iteration);
                return name;
            }
            ReviewVerdict::Retry(feedback) => {
                eprintln!("[loop] Verdict: RETRY");
                eprintln!("[loop] Feedback: {}", truncate_display(&feedback, 120));

                iteration += 1;
                let retry_message = format!(
                    "Read the implementation plan from {}.\n\
                     Check RESULT.md for validator feedback on previous attempts and address it.\n\
                     {result_instructions}",
                    plan_path.display()
                );

                eprintln!("\n[loop] === Attempt {iteration} ===");
                cmd_send(
                    &retry_message,
                    Some(&name),
                    model,
                    backend,
                    &[],
                    sandbox,
                    push,
                );
            }
        }
    }
}

fn main() {
    CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();
    let config = load_config();
    let dir_state = load_dir_state();

    let backend = cli.agent.unwrap_or_else(|| {
        if let Some(ref a) = dir_state.agent {
            match a.as_str() {
                "codex" => return Backend::Codex,
                "gemini" => return Backend::Gemini,
                "claude" => return Backend::Claude,
                _ => {}
            }
        }
        match config.agent.as_str() {
            "codex" => Backend::Codex,
            "gemini" => Backend::Gemini,
            _ => Backend::Claude,
        }
    });

    let sandbox_name: Option<String> = if cli.no_sandbox {
        None
    } else if let Some(name) = cli.sandbox {
        Some(name)
    } else if let Some(ref name) = dir_state.sandbox {
        Some(name.clone())
    } else if config.sandbox {
        Some(config.sandbox_name.clone())
    } else {
        None
    };
    let sandbox = sandbox_name.as_deref();

    let push = if cli.no_push { false } else { config.push };

    let save_after_send = |conversation: &str| {
        let mut state = load_dir_state();
        state.conversation = Some(conversation.to_string());
        state.agent = Some(backend_name(&backend).to_string());
        state.sandbox = sandbox.map(String::from);
        save_dir_state(&state);
        git_commit_state(push);
    };

    match (cli.message, cli.command) {
        (_, Some(Commands::New { name })) => cmd_new(&name, push),
        (_, Some(Commands::List)) => cmd_list(),
        (_, Some(Commands::Pick)) => cmd_pick(),
        (_, Some(Commands::Status)) => cmd_status(),
        (_, Some(Commands::Setup { shell })) => cmd_setup(&shell),
        (_, Some(Commands::Compact { name })) => cmd_compact(name.as_deref(), sandbox, push),
        (
            _,
            Some(Commands::Loop {
                plan,
                harness,
                agent: loop_agent,
                review_agent,
                review_model,
                conversation,
                files,
                sandbox: loop_sandbox,
                no_sandbox: loop_no_sandbox,
            }),
        ) => {
            // Resolve sandbox from loop-specific flags, falling back to global config
            let loop_sandbox_name: Option<String> = if loop_no_sandbox {
                None
            } else if let Some(name) = loop_sandbox {
                Some(name)
            } else {
                sandbox_name.clone()
            };
            let loop_sandbox_ref = loop_sandbox_name.as_deref();

            let impl_be = loop_agent.unwrap_or_else(|| backend.clone());
            let model_ref = cli.model.as_deref();
            let review_model_ref = review_model.as_deref().or(model_ref);
            let review_be = review_agent.unwrap_or_else(|| impl_be.clone());
            let target = conversation.as_deref().or(cli.conversation.as_deref());
            let name = cmd_loop(
                &plan,
                &harness,
                target,
                model_ref,
                &impl_be,
                review_model_ref,
                &review_be,
                &files,
                loop_sandbox_ref,
                push,
            );
            save_after_send(&name);
        }
        (Some(message), None) => {
            let name = cmd_send(
                &message,
                cli.conversation.as_deref(),
                cli.model.as_deref(),
                &backend,
                &cli.files,
                sandbox,
                push,
            );
            save_after_send(&name);
        }
        (None, None) => {
            // Try reading from stdin if it's piped
            if !io::stdin().is_terminal() {
                let mut input = String::new();
                io::stdin().read_to_string(&mut input).unwrap_or_default();
                let input = input.trim();
                if !input.is_empty() {
                    let name = cmd_send(
                        input,
                        cli.conversation.as_deref(),
                        cli.model.as_deref(),
                        &backend,
                        &cli.files,
                        sandbox,
                        push,
                    );
                    save_after_send(&name);
                    return;
                }
            }
            Cli::command().print_help().unwrap();
            println!();
        }
    }
}
