use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDateTime};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::engine::{
    ArgValueCandidates, ArgValueCompleter, CompletionCandidate, PathCompleter,
};
use clap_complete::env::CompleteEnv;
use serde::{Deserialize, Serialize};
use serenity::all::{ChannelId, GatewayIntents, UserId};
use serenity::async_trait;
use serenity::model::channel::Message as DiscordMessage;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use skim::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Cursor, IsTerminal, Read as _};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::time::{Duration, sleep};

#[derive(Deserialize)]
#[serde(default)]
struct Config {
    sandbox: bool,
    sandbox_name: String,
    push: bool,
    agent: String,
    discord_token: Option<String>,
    discord_guild_id: Option<String>,
    discord_allowed_users: Vec<String>,
    discord: Option<DiscordSection>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DiscordSection {
    #[serde(alias = "token")]
    bot_token: Option<String>,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    bots: HashMap<String, DiscordBotSection>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct DiscordBotSection {
    #[serde(alias = "token")]
    bot_token: Option<String>,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
}

#[derive(Clone)]
struct DiscordBotProfile {
    name: String,
    bot_token: String,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sandbox: true,
            sandbox_name: "default".into(),
            push: true,
            agent: "claude".into(),
            discord_token: None,
            discord_guild_id: None,
            discord_allowed_users: Vec::new(),
            discord: None,
        }
    }
}

impl Config {
    fn resolved_discord_token(&self) -> Option<String> {
        self.discord
            .as_ref()
            .and_then(|d| d.bot_token.clone())
            .or_else(|| self.discord_token.clone())
    }

    fn resolved_discord_guild_id(&self) -> Option<String> {
        self.discord
            .as_ref()
            .and_then(|d| d.guild_id.clone())
            .or_else(|| self.discord_guild_id.clone())
    }

    fn resolved_discord_allowed_users(&self) -> Vec<String> {
        let from_section = self
            .discord
            .as_ref()
            .map(|d| d.allowed_users.clone())
            .unwrap_or_default();
        if from_section.is_empty() {
            self.discord_allowed_users.clone()
        } else {
            from_section
        }
    }

    fn resolved_discord_profiles(&self) -> Vec<DiscordBotProfile> {
        let mut profiles = Vec::new();

        if let Some(discord) = &self.discord
            && !discord.bots.is_empty()
        {
            let mut named: Vec<(String, DiscordBotSection)> = discord
                .bots
                .iter()
                .map(|(name, section)| (name.clone(), section.clone()))
                .collect();
            named.sort_by(|a, b| a.0.cmp(&b.0));

            for (name, section) in named {
                if let Some(token) = section.bot_token {
                    profiles.push(DiscordBotProfile {
                        name,
                        bot_token: token,
                        guild_id: section.guild_id,
                        allowed_users: section.allowed_users,
                    });
                }
            }
            return profiles;
        }

        if let Some(token) = self.resolved_discord_token() {
            profiles.push(DiscordBotProfile {
                name: "default".to_string(),
                bot_token: token,
                guild_id: self.resolved_discord_guild_id(),
                allowed_users: self.resolved_discord_allowed_users(),
            });
        }

        profiles
    }

    fn find_discord_profile(&self, name: &str) -> Option<DiscordBotProfile> {
        self.resolved_discord_profiles()
            .into_iter()
            .find(|profile| profile.name == name)
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
    model: Option<String>,
    sandbox: Option<String>,
    discord_destination: Option<String>,
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

fn list_discord_bots() -> Vec<CompletionCandidate> {
    load_config()
        .resolved_discord_profiles()
        .into_iter()
        .map(|profile| {
            let help = profile
                .guild_id
                .as_deref()
                .map(|id| format!("Guild {id}"))
                .unwrap_or_else(|| "Guild (not set)".to_string());
            CompletionCandidate::new(profile.name).help(Some(help.into()))
        })
        .collect()
}

fn list_conversations() -> Vec<CompletionCandidate> {
    conversation_names_sorted()
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

fn conversation_names_sorted() -> Vec<String> {
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

#[derive(Clone, ValueEnum)]
enum Backend {
    Claude,
    Codex,
    Gemini,
}

#[derive(Parser)]
#[command(
    name = "breo",
    version,
    about = "Chat with an LLM, keeping conversation in a markdown file"
)]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    /// The message to send
    message: Option<String>,

    /// Send to a specific conversation and switch to it
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
    /// Rename a conversation
    Rename {
        /// Current conversation name (defaults to active)
        #[arg(add = ArgValueCandidates::new(list_conversations))]
        old_name: String,
        /// New name for the conversation
        new_name: String,
    },
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

        /// Path to the verification file (instructions for the validator)
        verification: PathBuf,

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
    /// Start the Discord bot bridge for the current directory
    #[command(long_about = "\
Start the Discord bot bridge for the current directory.

The bot responds to DMs and @mentions in channels. Messages are routed
through the same LLM backend as `breo send`, with full conversation
persistence.

Bot commands (send as a Discord message):
  !switch <name>    Switch to a different conversation
  !new <name>       Create a new conversation and switch to it
  !list             List all conversations
  !status           Show bot name, directory, conversation, agent, model, sandbox, destination
  !agent <backend>  Change the LLM backend (claude, codex, gemini, ...)
  !model <name>     Change the model
  !destination <channel_id|dm>  Change where responses are delivered
  !compact           Summarize the current conversation to save context

Scheduling:
  The bot also polls .breo/cron.toml every 10s and executes due tasks.
  One-shot tasks are removed after execution.
  Periodic tasks are rescheduled using next_run + interval.

Configuration:
  Bot profiles are defined in ~/.config/breo/config.toml under
  [discord.bots.<name>] with keys: bot_token, guild_id, allowed_users.
  Use `breo claws list` to see configured profiles.")]
    Claws {
        /// Bot profile name, or 'list' to show configured bot profiles
        #[arg(add = ArgValueCandidates::new(list_discord_bots))]
        bot: String,

        /// Override the LLM agent for this bot session
        #[arg(short, long, value_enum)]
        agent: Option<Backend>,

        /// Override the model for this bot session
        #[arg(short, long, add = ArgValueCandidates::new(list_models))]
        model: Option<String>,

        /// Lima instance name for sandbox
        #[arg(short, long)]
        sandbox: Option<String>,

        /// Discord guild ID (overrides profile config)
        #[arg(long)]
        guild_id: Option<String>,

        /// Discord response destination: channel ID or 'dm'
        #[arg(short = 'd', long)]
        destination: Option<String>,
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

    // 1. If explicitly set in state, use it
    if let Some(ref name) = state.conversation {
        let scoped = dir_conversations_dir().join(format!("{name}.md"));
        if scoped.exists() {
            return name.clone();
        }
        // Name set but file doesn't exist — fall through
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

fn create_conversation(name: &str, push: bool) -> Result<(), String> {
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
    git_commit_conversation(&path, &format!("breo: new conversation '{name}'"), push);
    git_commit_state(push);
    Ok(())
}

fn cmd_new(name: &str, push: bool) {
    if let Err(e) = create_conversation(name, push) {
        eprintln!("{e}");
        std::process::exit(1);
    }
    println!("Created and switched to conversation: {name}");
}

fn cmd_rename(old_name: &str, new_name: &str, push: bool) {
    let old_path = conversation_path(old_name);
    if !old_path.exists() {
        eprintln!("Conversation '{old_name}' does not exist");
        std::process::exit(1);
    }
    let new_path = conversation_path(new_name);
    if new_path.exists() {
        eprintln!("Conversation '{new_name}' already exists");
        std::process::exit(1);
    }

    if let Err(e) = fs::rename(&old_path, &new_path) {
        eprintln!(
            "Failed to rename {} -> {}: {e}",
            old_path.display(),
            new_path.display()
        );
        std::process::exit(1);
    }

    // Update the active conversation reference if it pointed to the old name
    let active = get_active();
    if active == old_name {
        set_active(new_name);
    }

    git_commit_conversation(
        &new_path,
        &format!("breo: rename '{old_name}' -> '{new_name}'"),
        push,
    );
    git_commit_state(push);

    println!("Renamed conversation: {old_name} -> {new_name}");
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
        set_active(&name);
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
    let entries = conversation_names_sorted();

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
    let model = state.model.as_deref().unwrap_or("(default)");
    let sandbox = state.sandbox.as_deref().unwrap_or("(not set)");
    let destination = state
        .discord_destination
        .as_deref()
        .unwrap_or("(default dm)");
    println!("directory:    {}", current_dir_key());
    println!("config:       {}", breo_dir().display());
    println!("conversations:{}", dir_conversations_dir().display());
    println!("conversation: {active}");
    println!("agent:        {agent}");
    println!("model:        {model}");
    println!("sandbox:      {sandbox}");
    println!("destination:  {destination}");
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
                        use io::Write;
                        let _ = io::stdout().flush();
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

fn backend_name(backend: &Backend) -> &'static str {
    match backend {
        Backend::Claude => "claude",
        Backend::Codex => "codex",
        Backend::Gemini => "gemini",
    }
}

fn backend_from_name(name: &str) -> Option<Backend> {
    match name.trim().to_lowercase().as_str() {
        "claude" => Some(Backend::Claude),
        "codex" => Some(Backend::Codex),
        "gemini" => Some(Backend::Gemini),
        _ => None,
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

fn persist_dir_state(
    conversation: &str,
    backend: &Backend,
    model: Option<&str>,
    sandbox: Option<&str>,
    discord_destination: Option<&str>,
    push: bool,
) {
    let mut state = load_dir_state();
    state.conversation = Some(conversation.to_string());
    state.agent = Some(backend_name(backend).to_string());
    state.model = model.map(ToString::to_string);
    state.sandbox = sandbox.map(ToString::to_string);
    state.discord_destination = discord_destination.map(ToString::to_string);
    save_dir_state(&state);
    git_commit_state(push);
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

const CRON_FILE_HEADER: &str = r#"# breo cron - scheduled messages for the claws Discord bot
#
# The claws bot polls this file every 10 seconds. When a task's next_run
# time has passed, the bot sends the message to breo (using the bot's active
# conversation and agent) and delivers the response to the bot's configured
# destination (channel or DM).
#
# Fields:
#   name       - unique task identifier
#   message    - the message to send to breo (the LLM agent will execute this)
#   next_run   - ISO 8601 datetime for the next execution (e.g. 2026-02-24T09:00:00)
#   interval   - (optional) duration between runs (e.g. "24h", "1h", "30m")
#                if omitted, the task runs once and is removed
#   status     - task state: "pending" (waiting), "running" (in progress)
#                do not manually set to "running" - the bot manages this
#
# One-shot tasks are removed after execution.
# Periodic tasks get next_run recalculated (next_run + interval) automatically.
# To schedule a task: write a [[task]] entry to this file, then reply to the
# user confirming the task has been scheduled (include the name, next_run time,
# and whether it is periodic or one-shot).
# To cancel a task, delete its [[task]] entry.
#
# Example:
#
#   [[task]]
#   name = "daily-status"
#   message = "Check the logs and report any errors from the last 24 hours"
#   next_run = 2026-02-24T09:00:00
#   interval = "24h"
#   status = "pending"
#
#   [[task]]
#   name = "one-shot-reminder"
#   message = "Summarize the current state of the feature branch"
#   next_run = 2026-02-24T15:30:00
#   status = "pending"
"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CronTaskStatus {
    Pending,
    Running,
}

#[derive(Clone)]
struct CronTask {
    name: String,
    message: String,
    next_run: NaiveDateTime,
    interval: Option<String>,
    status: CronTaskStatus,
}

fn cron_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".breo")
}

fn cron_file_path() -> PathBuf {
    cron_dir().join("cron.toml")
}

fn ensure_cron_file() {
    let dir = cron_dir();
    if !dir.exists()
        && let Err(e) = fs::create_dir_all(&dir)
    {
        eprintln!("Failed to create {}: {e}", dir.display());
        return;
    }

    let path = cron_file_path();
    if !path.exists()
        && let Err(e) = fs::write(&path, format!("{CRON_FILE_HEADER}\n"))
    {
        eprintln!("Failed to create {}: {e}", path.display());
    }
}

fn parse_timestamp(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc3339(text.trim())
                .ok()
                .map(|dt| dt.naive_local())
        })
}

fn parse_interval(text: &str) -> Option<ChronoDuration> {
    let trimmed = text.trim();
    if trimmed.len() < 2 {
        return None;
    }
    let (value, unit) = trimmed.split_at(trimmed.len() - 1);
    let amount: i64 = value.parse().ok()?;
    match unit {
        "s" => Some(ChronoDuration::seconds(amount)),
        "m" => Some(ChronoDuration::minutes(amount)),
        "h" => Some(ChronoDuration::hours(amount)),
        "d" => Some(ChronoDuration::days(amount)),
        _ => None,
    }
}

fn parse_cron_task(value: &toml::Value) -> Option<CronTask> {
    let table = value.as_table()?;
    let name = table.get("name")?.as_str()?.to_string();
    let message = table.get("message")?.as_str()?.to_string();
    let next_run_raw = table.get("next_run")?;
    let next_run_text = match next_run_raw {
        toml::Value::String(s) => s.clone(),
        toml::Value::Datetime(dt) => dt.to_string(),
        _ => return None,
    };
    let next_run = parse_timestamp(&next_run_text)?;
    let interval = table
        .get("interval")
        .and_then(toml::Value::as_str)
        .map(ToString::to_string);
    let status = match table
        .get("status")
        .and_then(toml::Value::as_str)
        .unwrap_or("pending")
        .to_lowercase()
        .as_str()
    {
        "running" => CronTaskStatus::Running,
        _ => CronTaskStatus::Pending,
    };

    Some(CronTask {
        name,
        message,
        next_run,
        interval,
        status,
    })
}

fn load_cron_tasks() -> Vec<CronTask> {
    ensure_cron_file();
    let path = cron_file_path();
    let Ok(contents) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = toml::from_str::<toml::Value>(&contents) else {
        eprintln!("[cron] Failed to parse {}.", path.display());
        return Vec::new();
    };

    value
        .get("task")
        .and_then(toml::Value::as_array)
        .map(|tasks| tasks.iter().filter_map(parse_cron_task).collect())
        .unwrap_or_default()
}

fn toml_quoted(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

fn save_cron_tasks(tasks: &[CronTask]) {
    ensure_cron_file();
    let path = cron_file_path();
    let mut out = String::new();
    out.push_str(CRON_FILE_HEADER);
    out.push('\n');

    for task in tasks {
        out.push_str("[[task]]\n");
        out.push_str(&format!("name = {}\n", toml_quoted(&task.name)));
        out.push_str(&format!("message = {}\n", toml_quoted(&task.message)));
        out.push_str(&format!(
            "next_run = {}\n",
            task.next_run.format("%Y-%m-%dT%H:%M:%S")
        ));
        if let Some(interval) = &task.interval {
            out.push_str(&format!("interval = {}\n", toml_quoted(interval)));
        }
        let status = match task.status {
            CronTaskStatus::Pending => "pending",
            CronTaskStatus::Running => "running",
        };
        out.push_str(&format!("status = {}\n\n", toml_quoted(status)));
    }

    if let Err(e) = fs::write(&path, out) {
        eprintln!("Failed to write {}: {e}", path.display());
    }
}

fn mark_task_running(task_name: &str) -> Option<CronTask> {
    let mut tasks = load_cron_tasks();
    let mut selected = None;
    let now = Local::now().naive_local();

    for task in &mut tasks {
        if task.name == task_name && task.status == CronTaskStatus::Pending && task.next_run <= now
        {
            task.status = CronTaskStatus::Running;
            selected = Some(task.clone());
            break;
        }
    }

    if selected.is_some() {
        save_cron_tasks(&tasks);
    }
    selected
}

fn complete_cron_task(task_name: &str, previous_next_run: NaiveDateTime, interval: Option<&str>) {
    let mut tasks = load_cron_tasks();

    if let Some(index) = tasks.iter().position(|task| task.name == task_name) {
        if let Some(interval_text) = interval {
            if let Some(step) = parse_interval(interval_text) {
                tasks[index].status = CronTaskStatus::Pending;
                tasks[index].next_run = previous_next_run + step;
            } else {
                eprintln!(
                    "[cron] Invalid interval '{}' for task '{}', removing task.",
                    interval_text, task_name
                );
                tasks.remove(index);
            }
        } else {
            tasks.remove(index);
        }
        save_cron_tasks(&tasks);
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_send_inner(
    message: &str,
    target: Option<&str>,
    model: Option<&str>,
    backend: &Backend,
    files: &[PathBuf],
    sandbox: Option<&str>,
    push: bool,
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
    let prompt = format!("{existing}## User\n{message}\n{attachments}");

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

    let content = format!("{prompt}\n## Assistant\n{response}\n\n");
    if let Err(e) = fs::write(&path, &content) {
        eprintln!("Failed to write {}: {e}", path.display());
        std::process::exit(1);
    }

    git_commit_conversation(&path, &format!("breo: message to '{name}'"), push);

    print_context_summary(&content, name, model, backend, &path);

    (name.to_string(), response.to_string(), true)
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
    let (name, stderr, success) =
        cmd_send_inner(message, target, model, backend, files, sandbox, push, true);
    if !success {
        let label = if sandbox.is_some() {
            "limactl"
        } else {
            backend_name(backend)
        };
        eprintln!("{label} failed: {stderr}");
        std::process::exit(1);
    }
    name
}

#[allow(clippy::too_many_arguments)]
fn cmd_loop(
    plan_path: &std::path::Path,
    verification_path: &std::path::Path,
    target: Option<&str>,
    model: Option<&str>,
    backend: &Backend,
    review_model: Option<&str>,
    review_backend: &Backend,
    files: &[PathBuf],
    sandbox: Option<&str>,
    push: bool,
) -> String {
    // Validate that plan and verification files are readable
    if let Err(e) = fs::metadata(plan_path) {
        eprintln!("Failed to read plan file {}: {e}", plan_path.display());
        std::process::exit(1);
    }
    if let Err(e) = fs::metadata(verification_path) {
        eprintln!(
            "Failed to read verification file {}: {e}",
            verification_path.display()
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
        "[loop] Plan: {} | Verification: {}",
        plan_path.display(),
        verification_path.display()
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

        // Validator shares the same conversation as the implementer
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
             Only return SUCCESS if the verification criteria are completely satisfied.",
            verification_path.display()
        );

        let (_, response_or_err, success) = cmd_send_inner(
            &review_message,
            Some(&name),
            review_model,
            review_backend,
            &[],
            sandbox,
            push,
            false,
        );

        if !success {
            let label = if sandbox.is_some() {
                "limactl"
            } else {
                backend_name(review_backend)
            };
            eprintln!("{label} failed during review: {response_or_err}");
            eprintln!("[loop] Stopping due to review error. Conversation: {name}");
            return name;
        }

        let response = response_or_err.trim();
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

fn split_for_discord(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return vec!["(no response)".to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(max_chars)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

fn strip_leading_mentions(input: &str) -> String {
    let mut rest = input.trim();
    loop {
        if let Some(after) = rest.strip_prefix("<@")
            && let Some(end) = after.find('>')
        {
            rest = after[end + 1..].trim_start();
            continue;
        }
        break;
    }
    rest.to_string()
}

fn parse_bot_command(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('!') {
        return None;
    }
    let raw = &trimmed[1..];
    let mut parts = raw.splitn(2, char::is_whitespace);
    let cmd = parts.next()?.trim().to_lowercase();
    let arg = parts.next().unwrap_or("").trim().to_string();
    Some((cmd, arg))
}

#[derive(Clone)]
enum DiscordDestination {
    Channel(String),
    Dm,
}

impl DiscordDestination {
    fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.eq_ignore_ascii_case("dm") {
            return Some(Self::Dm);
        }
        if !trimmed.is_empty() {
            return Some(Self::Channel(trimmed.to_string()));
        }
        None
    }

    fn to_storage(&self) -> String {
        match self {
            Self::Dm => "dm".to_string(),
            Self::Channel(id) => id.clone(),
        }
    }

    fn display(&self) -> String {
        match self {
            Self::Dm => "dm".to_string(),
            Self::Channel(id) => format!("channel {id}"),
        }
    }
}

#[derive(Clone)]
struct DiscordBotState {
    bot_name: String,
    conversation: String,
    backend: Backend,
    model: Option<String>,
    sandbox: Option<String>,
    destination: DiscordDestination,
    push: bool,
    allowed_users: Vec<String>,
    cron_started: bool,
}

struct ClawsHandler {
    state: Arc<tokio::sync::Mutex<DiscordBotState>>,
}

impl ClawsHandler {
    async fn send_text_to_source(
        &self,
        ctx: &Context,
        msg: &DiscordMessage,
        text: &str,
    ) -> serenity::Result<()> {
        for chunk in split_for_discord(text, 2000) {
            msg.channel_id.say(&ctx.http, chunk).await?;
        }
        Ok(())
    }

    async fn send_to_destination(
        http: &serenity::http::Http,
        destination: &DiscordDestination,
        allowed_users: &[String],
        text: &str,
    ) -> serenity::Result<()> {
        match destination {
            DiscordDestination::Channel(channel_id) => {
                let channel_id_num: u64 = match channel_id.parse() {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("[claws] Invalid channel destination '{}'", channel_id);
                        return Ok(());
                    }
                };
                for chunk in split_for_discord(text, 2000) {
                    ChannelId::new(channel_id_num).say(http, chunk).await?;
                }
            }
            DiscordDestination::Dm => {
                let Some(first_allowed) = allowed_users.first() else {
                    eprintln!("[claws] No allowed_users configured for DM destination");
                    return Ok(());
                };
                let user_id_num: u64 = match first_allowed.parse() {
                    Ok(id) => id,
                    Err(_) => {
                        eprintln!("[claws] Invalid allowed user id '{}'", first_allowed);
                        return Ok(());
                    }
                };
                let dm = UserId::new(user_id_num).create_dm_channel(http).await?;
                for chunk in split_for_discord(text, 2000) {
                    dm.say(http, chunk).await?;
                }
            }
        }
        Ok(())
    }

    async fn send_to_state_destination(&self, ctx: &Context, text: &str) -> serenity::Result<()> {
        let (destination, allowed_users) = {
            let state = self.state.lock().await;
            (state.destination.clone(), state.allowed_users.clone())
        };
        Self::send_to_destination(&ctx.http, &destination, &allowed_users, text).await
    }

    async fn handle_command(
        &self,
        ctx: &Context,
        msg: &DiscordMessage,
        command: &str,
        arg: &str,
    ) -> serenity::Result<()> {
        match command {
            "switch" => {
                if arg.is_empty() {
                    return self
                        .send_text_to_source(ctx, msg, "Usage: !switch <conversation>")
                        .await;
                }
                if !conversation_path(arg).exists() {
                    return self
                        .send_text_to_source(ctx, msg, "Conversation not found.")
                        .await;
                }
                let mut state = self.state.lock().await;
                state.conversation = arg.to_string();
                set_active(&state.conversation);
                persist_dir_state(
                    &state.conversation,
                    &state.backend,
                    state.model.as_deref(),
                    state.sandbox.as_deref(),
                    Some(&state.destination.to_storage()),
                    state.push,
                );
                drop(state);
                self.send_to_state_destination(
                    ctx,
                    &format!("Switched to conversation: {}", arg),
                )
                .await
            }
            "agent" => {
                if arg.is_empty() {
                    return self
                        .send_text_to_source(ctx, msg, "Usage: !agent <claude|codex|gemini>")
                        .await;
                }
                let Some(backend) = backend_from_name(arg) else {
                    return self
                        .send_text_to_source(
                            ctx,
                            msg,
                            "Unknown agent. Use: claude, codex, or gemini.",
                        )
                        .await;
                };
                let mut state = self.state.lock().await;
                state.backend = backend;
                persist_dir_state(
                    &state.conversation,
                    &state.backend,
                    state.model.as_deref(),
                    state.sandbox.as_deref(),
                    Some(&state.destination.to_storage()),
                    state.push,
                );
                let backend_name = backend_name(&state.backend).to_string();
                drop(state);
                self.send_to_state_destination(ctx, &format!("Switched to agent: {backend_name}"))
                    .await
            }
            "model" => {
                if arg.is_empty() {
                    return self
                        .send_text_to_source(ctx, msg, "Usage: !model <name>")
                        .await;
                }
                let mut state = self.state.lock().await;
                state.model = Some(arg.to_string());
                persist_dir_state(
                    &state.conversation,
                    &state.backend,
                    state.model.as_deref(),
                    state.sandbox.as_deref(),
                    Some(&state.destination.to_storage()),
                    state.push,
                );
                drop(state);
                self.send_to_state_destination(ctx, &format!("Switched to model: {}", arg))
                    .await
            }
            "destination" => {
                if arg.is_empty() {
                    return self
                        .send_text_to_source(ctx, msg, "Usage: !destination <channel_id|dm>")
                        .await;
                }
                let Some(new_destination) = DiscordDestination::parse(arg) else {
                    return self
                        .send_text_to_source(ctx, msg, "Invalid destination. Use channel ID or dm.")
                        .await;
                };
                let mut state = self.state.lock().await;
                state.destination = new_destination.clone();
                persist_dir_state(
                    &state.conversation,
                    &state.backend,
                    state.model.as_deref(),
                    state.sandbox.as_deref(),
                    Some(&state.destination.to_storage()),
                    state.push,
                );
                let destination_display = state.destination.display();
                drop(state);
                self.send_to_state_destination(
                    ctx,
                    &format!("Destination set to: {destination_display}"),
                )
                .await
            }
            "status" => {
                let state = self.state.lock().await;
                let dir = current_dir_key();
                let status = format!(
                    "bot: {}\ndirectory: {}\nconversation: {}\nagent: {}\nmodel: {}\nsandbox: {}\ndestination: {}",
                    state.bot_name,
                    dir,
                    state.conversation,
                    backend_name(&state.backend),
                    state.model.as_deref().unwrap_or("default"),
                    state.sandbox.as_deref().unwrap_or("none"),
                    state.destination.display(),
                );
                drop(state);
                self.send_to_state_destination(ctx, &status).await
            }
            "list" => {
                let state = self.state.lock().await;
                let names = conversation_names_sorted();
                if names.is_empty() {
                    return self
                        .send_to_state_destination(ctx, "No conversations yet.")
                        .await;
                }
                let body = names
                    .into_iter()
                    .map(|name| {
                        if name == state.conversation {
                            format!("* {name}")
                        } else {
                            format!("  {name}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                drop(state);
                self.send_to_state_destination(ctx, &body).await
            }
            "new" => {
                if arg.is_empty() {
                    return self
                        .send_text_to_source(ctx, msg, "Usage: !new <conversation>")
                        .await;
                }
                let state_snapshot = self.state.lock().await.clone();
                let conversation_name = arg.to_string();
                let result = tokio::task::spawn_blocking(move || {
                    create_conversation(&conversation_name, state_snapshot.push)
                })
                .await;

                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return self.send_text_to_source(ctx, msg, &e).await,
                    Err(e) => {
                        return self
                            .send_text_to_source(
                                ctx,
                                msg,
                                &format!("Conversation creation failed: {e}"),
                            )
                            .await;
                    }
                }

                let mut state = self.state.lock().await;
                state.conversation = arg.to_string();
                persist_dir_state(
                    &state.conversation,
                    &state.backend,
                    state.model.as_deref(),
                    state.sandbox.as_deref(),
                    Some(&state.destination.to_storage()),
                    state.push,
                );
                let conversation_name = state.conversation.clone();
                drop(state);
                self.send_to_state_destination(
                    ctx,
                    &format!("Created and switched to conversation: {conversation_name}"),
                )
                .await
            }
            "compact" => {
                let state = self.state.lock().await.clone();
                let conversation = state.conversation.clone();
                let path = conversation_path(&conversation);
                if !path.exists() {
                    return self
                        .send_text_to_source(ctx, msg, "Conversation does not exist.")
                        .await;
                }
                let content = fs::read_to_string(&path).unwrap_or_default();
                if count_exchanges(&content) == 0 {
                    return self
                        .send_to_state_destination(
                            ctx,
                            &format!("Nothing to compact in '{conversation}'"),
                        )
                        .await;
                }
                let conversation_for_msg = conversation.clone();
                let sandbox = state.sandbox.clone();
                let push = state.push;
                let compact_result = tokio::task::spawn_blocking(move || {
                    cmd_compact(Some(&conversation), sandbox.as_deref(), push);
                })
                .await;
                if let Err(e) = compact_result {
                    return self
                        .send_text_to_source(ctx, msg, &format!("Compaction failed: {e}"))
                        .await;
                }
                self.send_to_state_destination(
                    ctx,
                    &format!("Compacted conversation: {conversation_for_msg}"),
                )
                .await
            }
            _ => {
                self.send_text_to_source(
                    ctx,
                    msg,
                    "Unknown command. Use: !switch, !agent, !model, !destination, !status, !list, !new, !compact",
                )
                .await
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_cron_task(
    http: Arc<serenity::http::Http>,
    state: Arc<tokio::sync::Mutex<DiscordBotState>>,
    task: CronTask,
) {
    let state_snapshot = state.lock().await.clone();
    let conversation = state_snapshot.conversation;
    let backend = state_snapshot.backend;
    let model = state_snapshot.model;
    let sandbox = state_snapshot.sandbox;
    let destination = state_snapshot.destination;
    let allowed_users = state_snapshot.allowed_users;
    let push = state_snapshot.push;
    let task_name = task.name.clone();
    let task_message = task.message.clone();
    let interval = task.interval.clone();
    let previous_next_run = task.next_run;

    let send_result = tokio::task::spawn_blocking(move || {
        let (_, response_or_err, success) = cmd_send_inner(
            &task_message,
            Some(&conversation),
            model.as_deref(),
            &backend,
            &[],
            sandbox.as_deref(),
            push,
            false,
        );
        if success {
            Ok(response_or_err)
        } else {
            Err(response_or_err)
        }
    })
    .await;

    match send_result {
        Ok(Ok(response)) => {
            if let Err(e) =
                ClawsHandler::send_to_destination(&http, &destination, &allowed_users, &response)
                    .await
            {
                eprintln!("[cron] Failed to deliver task '{}' output: {e}", task_name);
            }
            complete_cron_task(&task_name, previous_next_run, interval.as_deref());
        }
        Ok(Err(err)) => {
            eprintln!(
                "[cron] Task '{}' failed: {}",
                task_name,
                truncate_display(&err, 120)
            );
            let _ = ClawsHandler::send_to_destination(
                &http,
                &destination,
                &allowed_users,
                &format!("Cron task '{task_name}' failed: {err}"),
            )
            .await;
            complete_cron_task(&task_name, previous_next_run, interval.as_deref());
        }
        Err(err) => {
            eprintln!("[cron] Worker failure for task '{}': {err}", task_name);
            complete_cron_task(&task_name, previous_next_run, interval.as_deref());
        }
    }
}

async fn cron_poll_loop(
    http: Arc<serenity::http::Http>,
    state: Arc<tokio::sync::Mutex<DiscordBotState>>,
) {
    loop {
        ensure_cron_file();
        let now = Local::now().naive_local();
        let due_pending_names: Vec<String> = load_cron_tasks()
            .into_iter()
            .filter(|task| task.status == CronTaskStatus::Pending && task.next_run <= now)
            .map(|task| task.name)
            .collect();

        for task_name in due_pending_names {
            if let Some(task) = mark_task_running(&task_name) {
                execute_cron_task(http.clone(), state.clone(), task).await;
            }
        }

        sleep(Duration::from_secs(10)).await;
    }
}

#[async_trait]
impl EventHandler for ClawsHandler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        eprintln!("[claws] Connected as {}", ready.user.name);
        ensure_cron_file();

        let should_start = {
            let mut state = self.state.lock().await;
            if state.cron_started {
                false
            } else {
                state.cron_started = true;
                true
            }
        };

        if should_start {
            tokio::spawn(cron_poll_loop(ctx.http.clone(), self.state.clone()));
            eprintln!("[cron] Polling {}", cron_file_path().display());
        }
    }

    async fn message(&self, ctx: Context, msg: DiscordMessage) {
        if msg.author.bot {
            return;
        }

        let is_dm = msg.guild_id.is_none();
        let is_mention = msg.mentions_me(&ctx).await.unwrap_or(false);
        if !is_dm && !is_mention {
            return;
        }

        let user_id = msg.author.id.get().to_string();
        let allowed_users = {
            let state = self.state.lock().await;
            state.allowed_users.clone()
        };
        if !allowed_users.contains(&user_id) {
            let _ = self.send_text_to_source(&ctx, &msg, "Access denied.").await;
            return;
        }

        let content = strip_leading_mentions(&msg.content);
        if content.is_empty() {
            return;
        }

        if let Some((command, arg)) = parse_bot_command(&content) {
            let _ = msg.channel_id.broadcast_typing(&ctx.http).await;
            let _ = self.handle_command(&ctx, &msg, &command, &arg).await;
            return;
        }

        let _ = msg.channel_id.broadcast_typing(&ctx.http).await;

        let state = self.state.lock().await.clone();
        let conversation = state.conversation.clone();
        let backend = state.backend.clone();
        let model = state.model.clone();
        let sandbox = state.sandbox.clone();
        let push = state.push;
        let message = content.clone();

        let result = tokio::task::spawn_blocking(move || {
            let (_, response_or_err, success) = cmd_send_inner(
                &message,
                Some(&conversation),
                model.as_deref(),
                &backend,
                &[],
                sandbox.as_deref(),
                push,
                false,
            );
            if success {
                Ok(response_or_err)
            } else {
                Err(response_or_err)
            }
        })
        .await;

        match result {
            Ok(Ok(response)) => {
                let _ = self.send_to_state_destination(&ctx, &response).await;
            }
            Ok(Err(err)) => {
                let _ = self
                    .send_to_state_destination(&ctx, &format!("Command failed: {err}"))
                    .await;
            }
            Err(err) => {
                let _ = self
                    .send_to_state_destination(&ctx, &format!("Worker failed: {err}"))
                    .await;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_claws(
    bot_name: &str,
    token: &str,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    backend: Backend,
    model: Option<String>,
    sandbox: Option<String>,
    destination: DiscordDestination,
    push: bool,
) {
    ensure_breo_dir();
    let conversation = get_active();
    let intents = GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    eprintln!(
        "[claws] Bot: {} | Conversation: {} | Agent: {} | Model: {} | Sandbox: {} | Destination: {}",
        bot_name,
        conversation,
        backend_name(&backend),
        model.as_deref().unwrap_or("default"),
        sandbox.as_deref().unwrap_or("none"),
        destination.display()
    );
    if let Some(id) = guild_id.as_deref() {
        eprintln!("[claws] Guild filter configured: {id}");
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create async runtime: {e}");
            std::process::exit(1);
        }
    };

    runtime.block_on(async move {
        let state = Arc::new(tokio::sync::Mutex::new(DiscordBotState {
            bot_name: bot_name.to_string(),
            conversation,
            backend,
            model,
            sandbox,
            destination,
            push,
            allowed_users,
            cron_started: false,
        }));

        let handler = ClawsHandler {
            state: state.clone(),
        };

        let mut client = match serenity::Client::builder(token, intents)
            .event_handler(handler)
            .await
        {
            Ok(client) => client,
            Err(e) => {
                eprintln!("Failed to create Discord client: {e}");
                std::process::exit(1);
            }
        };

        let shard_manager = client.shard_manager.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            eprintln!("[claws] Shutdown requested");
            shard_manager.shutdown_all().await;
        });

        if let Err(e) = client.start().await {
            eprintln!("Discord client exited with error: {e}");
            std::process::exit(1);
        }
    });
}

fn cmd_claws_list(config: &Config) {
    let profiles = config.resolved_discord_profiles();
    if profiles.is_empty() {
        eprintln!("No Discord bot profiles configured.");
        eprintln!("Add entries under [discord.bots.<name>] in config.toml.");
        std::process::exit(1);
    }

    for profile in profiles {
        println!(
            "{}\tguild={}\tallowed_users={}",
            profile.name,
            profile.guild_id.as_deref().unwrap_or("(none)"),
            profile.allowed_users.len()
        );
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

    // Model resolution: CLI --model > directory state > backend default (None)
    let resolved_model: Option<String> = cli.model.clone().or_else(|| dir_state.model.clone());

    let save_after_send = |conversation: &str| {
        persist_dir_state(
            conversation,
            &backend,
            resolved_model.as_deref(),
            sandbox,
            dir_state.discord_destination.as_deref(),
            push,
        );
    };

    match (cli.message, cli.command) {
        (_, Some(Commands::New { name })) => cmd_new(&name, push),
        (_, Some(Commands::Rename { old_name, new_name })) => {
            cmd_rename(&old_name, &new_name, push)
        }
        (_, Some(Commands::List)) => cmd_list(),
        (_, Some(Commands::Pick)) => cmd_pick(),
        (_, Some(Commands::Status)) => cmd_status(),
        (_, Some(Commands::Setup { shell })) => cmd_setup(&shell),
        (_, Some(Commands::Compact { name })) => cmd_compact(name.as_deref(), sandbox, push),
        (
            _,
            Some(Commands::Claws {
                bot,
                agent: claws_agent,
                model: claws_model,
                sandbox: claws_sandbox,
                guild_id: claws_guild_id,
                destination: claws_destination,
            }),
        ) => {
            if bot == "list" {
                cmd_claws_list(&config);
                return;
            }

            let profile = config.find_discord_profile(&bot).unwrap_or_else(|| {
                eprintln!("Discord bot profile '{bot}' was not found.");
                eprintln!("Use `breo claws list` to view configured bot profiles.");
                std::process::exit(1);
            });

            if profile.allowed_users.is_empty() {
                eprintln!(
                    "Profile '{bot}' has no allowed users.\n\
                     Add `allowed_users = [\"...\"]` under [discord.bots.{bot}] in config.toml."
                );
                std::process::exit(1);
            }

            // CLI flags override global config for claws session
            let claws_backend = claws_agent.unwrap_or(backend);
            let claws_resolved_model = claws_model.or(resolved_model);
            let claws_sandbox_name = claws_sandbox.or(sandbox_name);
            let claws_guild = claws_guild_id.or(profile.guild_id);
            let claws_destination = claws_destination
                .as_deref()
                .map(|value| {
                    DiscordDestination::parse(value).unwrap_or_else(|| {
                        eprintln!(
                            "Invalid --destination value '{value}'. Use 'dm' or a Discord channel ID."
                        );
                        std::process::exit(1);
                    })
                })
                .or_else(|| {
                    dir_state
                        .discord_destination
                        .as_deref()
                        .and_then(DiscordDestination::parse)
                })
                .unwrap_or(DiscordDestination::Dm);

            cmd_claws(
                &profile.name,
                &profile.bot_token,
                claws_guild,
                profile.allowed_users,
                claws_backend,
                claws_resolved_model,
                claws_sandbox_name,
                claws_destination,
                push,
            );
        }
        (
            _,
            Some(Commands::Loop {
                plan,
                verification,
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
            let model_ref = resolved_model.as_deref();
            let review_model_ref = review_model.as_deref().or(model_ref);
            let review_be = review_agent.unwrap_or_else(|| impl_be.clone());
            let target = conversation.as_deref().or(cli.conversation.as_deref());
            let name = cmd_loop(
                &plan,
                &verification,
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
                resolved_model.as_deref(),
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
                        resolved_model.as_deref(),
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
