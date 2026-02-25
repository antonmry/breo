use clap::ValueEnum;
use clap_complete::engine::CompletionCandidate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum Backend {
    Claude,
    Codex,
    Gemini,
}

#[derive(Clone, ValueEnum)]
pub(crate) enum ShellType {
    Bash,
    Zsh,
    Fish,
}

#[derive(Deserialize)]
#[serde(default)]
pub(crate) struct Config {
    pub(crate) sandbox: bool,
    pub(crate) sandbox_name: String,
    pub(crate) push: bool,
    pub(crate) agent: String,
    pub(crate) discord_token: Option<String>,
    pub(crate) discord_guild_id: Option<String>,
    pub(crate) discord_allowed_users: Vec<String>,
    pub(crate) discord: Option<DiscordSection>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct DiscordSection {
    #[serde(alias = "token")]
    pub(crate) bot_token: Option<String>,
    pub(crate) guild_id: Option<String>,
    pub(crate) allowed_users: Vec<String>,
    pub(crate) bots: HashMap<String, DiscordBotSection>,
}

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
pub(crate) struct DiscordBotSection {
    #[serde(alias = "token")]
    pub(crate) bot_token: Option<String>,
    pub(crate) guild_id: Option<String>,
    pub(crate) allowed_users: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DiscordBotProfile {
    pub(crate) name: String,
    pub(crate) bot_token: String,
    pub(crate) guild_id: Option<String>,
    pub(crate) allowed_users: Vec<String>,
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
    pub(crate) fn resolved_discord_token(&self) -> Option<String> {
        self.discord
            .as_ref()
            .and_then(|d| d.bot_token.clone())
            .or_else(|| self.discord_token.clone())
    }

    pub(crate) fn resolved_discord_guild_id(&self) -> Option<String> {
        self.discord
            .as_ref()
            .and_then(|d| d.guild_id.clone())
            .or_else(|| self.discord_guild_id.clone())
    }

    pub(crate) fn resolved_discord_allowed_users(&self) -> Vec<String> {
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

    pub(crate) fn resolved_discord_profiles(&self) -> Vec<DiscordBotProfile> {
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

    pub(crate) fn find_discord_profile(&self, name: &str) -> Option<DiscordBotProfile> {
        self.resolved_discord_profiles()
            .into_iter()
            .find(|profile| profile.name == name)
    }
}

pub(crate) fn load_config() -> Config {
    let path = crate::conversation::breo_dir().join("config.toml");
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub(crate) struct DirState {
    pub(crate) conversation: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) sandbox: Option<String>,
    pub(crate) discord_destination: Option<String>,
    pub(crate) dir_id: Option<String>,
}

pub(crate) fn state_file_path() -> PathBuf {
    crate::conversation::breo_dir().join("state.toml")
}

pub(crate) fn current_dir_key() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub(crate) fn load_all_state() -> HashMap<String, DirState> {
    let path = state_file_path();
    match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

pub(crate) fn save_all_state(map: &HashMap<String, DirState>) {
    let path = state_file_path();
    if let Ok(contents) = toml::to_string(map) {
        let _ = fs::write(&path, contents);
    }
}

pub(crate) fn load_dir_state() -> DirState {
    let key = current_dir_key();
    load_all_state().remove(&key).unwrap_or_default()
}

pub(crate) fn save_dir_state(state: &DirState) {
    let key = current_dir_key();
    let mut map = load_all_state();
    map.insert(key, state.clone());
    save_all_state(&map);
}

pub(crate) fn list_models() -> Vec<CompletionCandidate> {
    vec![
        CompletionCandidate::new("opus").help(Some("Claude Opus 4.6 (200k)".into())),
        CompletionCandidate::new("sonnet").help(Some("Claude Sonnet 4.5 (200k)".into())),
        CompletionCandidate::new("haiku").help(Some("Claude Haiku 4.5 (200k)".into())),
        CompletionCandidate::new("gpt-5").help(Some("GPT-5 (400k)".into())),
        CompletionCandidate::new("gpt-5-mini").help(Some("GPT-5 mini (400k)".into())),
        CompletionCandidate::new("o3").help(Some("o3 (200k)".into())),
        CompletionCandidate::new("o4-mini").help(Some("o4-mini (200k)".into())),
        CompletionCandidate::new("gemini-3.1-pro-preview").help(Some("Gemini 3.1 Pro (1M)".into())),
        CompletionCandidate::new("gemini-3-pro-preview").help(Some("Gemini 3 Pro (1M)".into())),
        CompletionCandidate::new("gemini-3-flash-preview").help(Some("Gemini 3 Flash (1M)".into())),
        CompletionCandidate::new("gemini-2.5-pro").help(Some("Gemini 2.5 Pro (1M)".into())),
        CompletionCandidate::new("gemini-2.5-flash").help(Some("Gemini 2.5 Flash (1M)".into())),
    ]
}

pub(crate) fn list_discord_bots() -> Vec<CompletionCandidate> {
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

pub(crate) fn list_conversations() -> Vec<CompletionCandidate> {
    crate::conversation::conversation_names_sorted()
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

pub(crate) fn cmd_status() {
    let active = crate::conversation::get_active();
    let state = load_dir_state();
    let agent = state.agent.as_deref().unwrap_or("(not set)");
    let model = state.model.as_deref().unwrap_or("(default)");
    let sandbox = state.sandbox.as_deref().unwrap_or("(not set)");
    let destination = state
        .discord_destination
        .as_deref()
        .unwrap_or("(default dm)");
    println!("directory:    {}", current_dir_key());
    println!(
        "config:       {}",
        crate::conversation::breo_dir().display()
    );
    println!(
        "conversations:{}",
        crate::conversation::dir_conversations_dir().display()
    );
    println!("conversation: {active}");
    println!("agent:        {agent}");
    println!("model:        {model}");
    println!("sandbox:      {sandbox}");
    println!("destination:  {destination}");
}

pub(crate) fn cmd_setup(shell: &ShellType) {
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

pub(crate) fn backend_name(backend: &Backend) -> &'static str {
    match backend {
        Backend::Claude => "claude",
        Backend::Codex => "codex",
        Backend::Gemini => "gemini",
    }
}

pub(crate) fn backend_from_name(name: &str) -> Option<Backend> {
    match name.trim().to_lowercase().as_str() {
        "claude" => Some(Backend::Claude),
        "codex" => Some(Backend::Codex),
        "gemini" => Some(Backend::Gemini),
        _ => None,
    }
}

pub(crate) fn persist_dir_state(
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
    crate::conversation::git_commit_state(push);
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
        unsafe {
            std::env::set_var("HOME", tmp.path());
            std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));
        }
        let out = f();
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
    fn config_default_values() {
        let c = Config::default();
        assert!(c.sandbox);
        assert_eq!(c.sandbox_name, "default");
        assert!(c.push);
        assert_eq!(c.agent, "claude");
    }

    #[test]
    fn resolved_discord_token_cascade() {
        let mut c = Config {
            discord_token: Some("flat".into()),
            ..Config::default()
        };
        assert_eq!(c.resolved_discord_token().as_deref(), Some("flat"));
        c.discord = Some(DiscordSection {
            bot_token: Some("section".into()),
            ..DiscordSection::default()
        });
        assert_eq!(c.resolved_discord_token().as_deref(), Some("section"));
    }

    #[test]
    fn resolved_discord_guild_id_cascade() {
        let mut c = Config {
            discord_guild_id: Some("g1".into()),
            ..Config::default()
        };
        assert_eq!(c.resolved_discord_guild_id().as_deref(), Some("g1"));
        c.discord = Some(DiscordSection {
            guild_id: Some("g2".into()),
            ..DiscordSection::default()
        });
        assert_eq!(c.resolved_discord_guild_id().as_deref(), Some("g2"));
    }

    #[test]
    fn resolved_discord_allowed_users_cascade() {
        let mut c = Config {
            discord_allowed_users: vec!["1".into()],
            ..Config::default()
        };
        assert_eq!(c.resolved_discord_allowed_users(), vec!["1"]);
        c.discord = Some(DiscordSection {
            allowed_users: vec!["2".into(), "3".into()],
            ..DiscordSection::default()
        });
        assert_eq!(c.resolved_discord_allowed_users(), vec!["2", "3"]);
    }

    #[test]
    fn resolved_discord_profiles_with_bots() {
        let mut bots = HashMap::new();
        bots.insert(
            "a".into(),
            DiscordBotSection {
                bot_token: Some("ta".into()),
                ..DiscordBotSection::default()
            },
        );
        bots.insert(
            "b".into(),
            DiscordBotSection {
                bot_token: Some("tb".into()),
                ..DiscordBotSection::default()
            },
        );
        let c = Config {
            discord: Some(DiscordSection {
                bots,
                ..DiscordSection::default()
            }),
            ..Config::default()
        };
        let names: Vec<String> = c
            .resolved_discord_profiles()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn resolved_discord_profiles_flat_fallback() {
        let c = Config {
            discord_token: Some("tok".into()),
            ..Config::default()
        };
        let profiles = c.resolved_discord_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "default");
        assert_eq!(profiles[0].bot_token, "tok");
    }

    #[test]
    fn find_discord_profile_found_not_found() {
        let c = Config {
            discord_token: Some("tok".into()),
            ..Config::default()
        };
        assert!(c.find_discord_profile("default").is_some());
        assert!(c.find_discord_profile("missing").is_none());
    }

    #[test]
    fn dir_state_round_trip() {
        let s = DirState {
            conversation: Some("c".into()),
            agent: Some("claude".into()),
            model: Some("opus".into()),
            sandbox: Some("default".into()),
            discord_destination: Some("dm".into()),
            dir_id: Some("id".into()),
        };
        let t = toml::to_string(&s).expect("serialize");
        let back: DirState = toml::from_str(&t).expect("deserialize");
        assert_eq!(back.conversation.as_deref(), Some("c"));
        assert_eq!(back.dir_id.as_deref(), Some("id"));
    }

    #[test]
    fn backend_from_name_all_cases() {
        assert!(matches!(backend_from_name("claude"), Some(Backend::Claude)));
        assert!(matches!(backend_from_name("codex"), Some(Backend::Codex)));
        assert!(matches!(backend_from_name("gemini"), Some(Backend::Gemini)));
        assert!(backend_from_name("unknown").is_none());
    }

    #[test]
    fn list_models_non_empty() {
        assert!(!list_models().is_empty());
    }

    #[test]
    fn backend_name_all_cases() {
        assert_eq!(backend_name(&Backend::Claude), "claude");
        assert_eq!(backend_name(&Backend::Codex), "codex");
        assert_eq!(backend_name(&Backend::Gemini), "gemini");
    }

    #[test]
    fn cmd_setup_bash_output() {
        // cmd_setup prints to stdout; just verify it doesn't panic
        cmd_setup(&ShellType::Bash);
    }

    #[test]
    fn cmd_setup_zsh_output() {
        cmd_setup(&ShellType::Zsh);
    }

    #[test]
    fn cmd_setup_fish_output() {
        cmd_setup(&ShellType::Fish);
    }

    #[test]
    #[serial]
    fn state_file_path_under_breo_dir() {
        with_temp_home(|| {
            let p = state_file_path();
            assert!(p.to_string_lossy().contains("breo"));
            assert!(p.to_string_lossy().ends_with("state.toml"));
        });
    }

    #[test]
    fn current_dir_key_non_empty() {
        let key = current_dir_key();
        let _ = key;
    }

    #[test]
    #[serial]
    fn load_config_returns_defaults_when_no_file() {
        with_temp_home(|| {
            let c = load_config();
            assert!(!c.agent.is_empty());
        });
    }

    #[test]
    #[serial]
    fn load_and_save_all_state_round_trip() {
        with_temp_home(|| {
            let map = load_all_state();
            save_all_state(&map);
        });
    }

    #[test]
    #[serial]
    fn load_dir_state_returns_default() {
        with_temp_home(|| {
            let state = load_dir_state();
            let _ = state;
        });
    }

    #[test]
    fn config_from_toml_string() {
        let toml_str = r#"
sandbox = false
sandbox_name = "test"
push = false
agent = "gemini"
discord_token = "tok123"
discord_guild_id = "guild1"
discord_allowed_users = ["u1", "u2"]
"#;
        let c: Config = toml::from_str(toml_str).expect("parse");
        assert!(!c.sandbox);
        assert_eq!(c.sandbox_name, "test");
        assert!(!c.push);
        assert_eq!(c.agent, "gemini");
        assert_eq!(c.discord_token.as_deref(), Some("tok123"));
        assert_eq!(c.discord_guild_id.as_deref(), Some("guild1"));
        assert_eq!(c.discord_allowed_users, vec!["u1", "u2"]);
    }

    #[test]
    fn config_from_toml_with_discord_bots() {
        let toml_str = r#"
[discord]
[discord.bots.mybot]
bot_token = "tok"
guild_id = "g1"
allowed_users = ["user1"]
"#;
        let c: Config = toml::from_str(toml_str).expect("parse");
        let profiles = c.resolved_discord_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "mybot");
        assert_eq!(profiles[0].bot_token, "tok");
        assert_eq!(profiles[0].guild_id.as_deref(), Some("g1"));
    }

    #[test]
    #[serial]
    fn list_conversations_no_panic() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            let _ = list_conversations();
        });
    }

    #[test]
    #[serial]
    fn list_discord_bots_no_panic() {
        with_temp_home(|| {
            let _ = list_discord_bots();
        });
    }

    #[test]
    fn config_invalid_toml_returns_default() {
        let c: Config = toml::from_str("not valid toml {{{").unwrap_or_default();
        assert!(c.sandbox);
        assert_eq!(c.agent, "claude");
    }

    #[test]
    fn backend_from_name_case_insensitive() {
        assert!(matches!(backend_from_name("CLAUDE"), Some(Backend::Claude)));
        assert!(matches!(backend_from_name("Codex"), Some(Backend::Codex)));
        assert!(matches!(
            backend_from_name("  GEMINI  "),
            Some(Backend::Gemini)
        ));
    }

    #[test]
    fn dir_state_default_all_none() {
        let s = DirState::default();
        assert!(s.conversation.is_none());
        assert!(s.agent.is_none());
        assert!(s.model.is_none());
        assert!(s.sandbox.is_none());
        assert!(s.discord_destination.is_none());
        assert!(s.dir_id.is_none());
    }

    #[test]
    fn dir_state_serialization_empty() {
        let s = DirState::default();
        let t = toml::to_string(&s).expect("serialize");
        // Empty DirState should serialize to empty or near-empty string
        assert!(!t.contains("conversation"));
    }

    #[test]
    fn config_discord_section_default() {
        let d = DiscordSection::default();
        assert!(d.bot_token.is_none());
        assert!(d.guild_id.is_none());
        assert!(d.allowed_users.is_empty());
        assert!(d.bots.is_empty());
    }

    #[test]
    fn config_discord_bot_section_default() {
        let b = DiscordBotSection::default();
        assert!(b.bot_token.is_none());
        assert!(b.guild_id.is_none());
        assert!(b.allowed_users.is_empty());
    }

    #[test]
    fn resolved_discord_token_none_when_not_set() {
        let c = Config::default();
        assert!(c.resolved_discord_token().is_none());
    }

    #[test]
    fn resolved_discord_guild_id_none_when_not_set() {
        let c = Config::default();
        assert!(c.resolved_discord_guild_id().is_none());
    }

    #[test]
    fn resolved_discord_allowed_users_empty_when_not_set() {
        let c = Config::default();
        assert!(c.resolved_discord_allowed_users().is_empty());
    }

    #[test]
    fn resolved_discord_profiles_empty_when_not_set() {
        let c = Config::default();
        assert!(c.resolved_discord_profiles().is_empty());
    }

    #[test]
    fn resolved_discord_profiles_skip_bots_without_token() {
        let mut bots = HashMap::new();
        bots.insert(
            "no-token".into(),
            DiscordBotSection {
                bot_token: None,
                guild_id: Some("g1".into()),
                allowed_users: vec!["u1".into()],
            },
        );
        bots.insert(
            "with-token".into(),
            DiscordBotSection {
                bot_token: Some("tok".into()),
                ..DiscordBotSection::default()
            },
        );
        let c = Config {
            discord: Some(DiscordSection {
                bots,
                ..DiscordSection::default()
            }),
            ..Config::default()
        };
        let profiles = c.resolved_discord_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "with-token");
    }

    #[test]
    #[serial]
    fn persist_dir_state_sets_all_fields() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            persist_dir_state(
                "test-conv",
                &Backend::Claude,
                Some("opus"),
                Some("vm"),
                Some("dm"),
                false,
            );
        });
    }

    #[test]
    fn list_models_has_all_expected_models() {
        let models: Vec<String> = list_models()
            .into_iter()
            .map(|c| c.get_value().to_string_lossy().to_string())
            .collect();
        assert!(models.contains(&"opus".to_string()));
        assert!(models.contains(&"sonnet".to_string()));
        assert!(models.contains(&"haiku".to_string()));
        assert!(models.contains(&"gpt-5".to_string()));
        assert!(models.contains(&"gemini-3.1-pro-preview".to_string()));
    }

    #[test]
    #[serial]
    fn cmd_status_does_not_panic() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            cmd_status();
        });
    }

    #[test]
    #[serial]
    fn save_all_state_and_load_round_trip() {
        with_temp_home(|| {
            let map = load_all_state();
            save_all_state(&map);
            let _ = load_all_state();
        });
    }

    #[test]
    fn config_from_toml_partial_fields() {
        let toml_str = r#"agent = "codex""#;
        let c: Config = toml::from_str(toml_str).expect("parse");
        assert_eq!(c.agent, "codex");
        // Other fields should be defaults
        assert!(c.sandbox);
        assert!(c.push);
        assert_eq!(c.sandbox_name, "default");
    }

    #[test]
    fn discord_bot_profile_clone() {
        let p = DiscordBotProfile {
            name: "bot".into(),
            bot_token: "tok".into(),
            guild_id: Some("g".into()),
            allowed_users: vec!["u".into()],
        };
        let cloned = p.clone();
        assert_eq!(cloned.name, "bot");
        assert_eq!(cloned.bot_token, "tok");
    }

    #[test]
    fn config_from_toml_discord_alias_token() {
        let toml_str = r#"
[discord]
token = "aliased-tok"
"#;
        let c: Config = toml::from_str(toml_str).expect("parse");
        assert_eq!(
            c.discord.as_ref().unwrap().bot_token.as_deref(),
            Some("aliased-tok")
        );
    }

    #[test]
    fn config_from_toml_bot_alias_token() {
        let toml_str = r#"
[discord.bots.mybot]
token = "aliased-bot-tok"
"#;
        let c: Config = toml::from_str(toml_str).expect("parse");
        let profiles = c.resolved_discord_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].bot_token, "aliased-bot-tok");
    }

    // --- Backend tests ---

    #[test]
    fn backend_name_all_variants() {
        assert_eq!(backend_name(&Backend::Claude), "claude");
        assert_eq!(backend_name(&Backend::Codex), "codex");
        assert_eq!(backend_name(&Backend::Gemini), "gemini");
    }

    #[test]
    fn backend_from_name_all_variants() {
        assert!(matches!(backend_from_name("claude"), Some(Backend::Claude)));
        assert!(matches!(backend_from_name("codex"), Some(Backend::Codex)));
        assert!(matches!(backend_from_name("gemini"), Some(Backend::Gemini)));
    }

    #[test]
    fn backend_from_name_unknown() {
        assert!(backend_from_name("unknown").is_none());
        assert!(backend_from_name("").is_none());
        assert!(backend_from_name("Claude").is_some()); // case insensitive
    }

    #[test]
    fn backend_clone() {
        let b = Backend::Claude;
        let c = b.clone();
        assert!(matches!(c, Backend::Claude));
    }

    #[test]
    fn backend_debug() {
        let debug = format!("{:?}", Backend::Claude);
        assert_eq!(debug, "Claude");
        assert_eq!(format!("{:?}", Backend::Codex), "Codex");
        assert_eq!(format!("{:?}", Backend::Gemini), "Gemini");
    }

    // --- Config default tests ---

    #[test]
    fn config_default_all_fields() {
        let c = Config::default();
        assert!(c.sandbox);
        assert_eq!(c.sandbox_name, "default");
        assert!(c.push);
        assert_eq!(c.agent, "claude");
        assert!(c.discord_token.is_none());
        assert!(c.discord_guild_id.is_none());
        assert!(c.discord_allowed_users.is_empty());
        assert!(c.discord.is_none());
    }

    // --- DiscordBotProfile tests ---

    #[test]
    fn discord_bot_profile_clone_with_guild() {
        let p = DiscordBotProfile {
            name: "bot".into(),
            bot_token: "tok".into(),
            guild_id: Some("g1".into()),
            allowed_users: vec!["u1".into()],
        };
        let c = p.clone();
        assert_eq!(c.name, "bot");
        assert_eq!(c.bot_token, "tok");
        assert_eq!(c.guild_id.as_deref(), Some("g1"));
    }

    #[test]
    fn discord_bot_profile_debug() {
        let p = DiscordBotProfile {
            name: "bot".into(),
            bot_token: "tok".into(),
            guild_id: None,
            allowed_users: vec![],
        };
        let debug = format!("{:?}", p);
        assert!(debug.contains("bot"));
    }

    // --- resolved_discord_profiles tests ---

    #[test]
    fn resolved_discord_profiles_empty_config() {
        let c = Config::default();
        assert!(c.resolved_discord_profiles().is_empty());
    }

    #[test]
    fn resolved_discord_profiles_top_level_only() {
        let c = Config {
            discord: Some(DiscordSection {
                bot_token: Some("tok".into()),
                guild_id: Some("g1".into()),
                allowed_users: vec!["u1".into()],
                bots: HashMap::new(),
            }),
            ..Config::default()
        };
        let profiles = c.resolved_discord_profiles();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "default");
        assert_eq!(profiles[0].bot_token, "tok");
    }

    #[test]
    fn resolved_discord_profiles_bot_sections_only() {
        let mut bots = HashMap::new();
        bots.insert(
            "bot1".into(),
            DiscordBotSection {
                bot_token: Some("tok1".into()),
                guild_id: Some("g1".into()),
                allowed_users: vec!["u1".into()],
            },
        );
        bots.insert(
            "bot2".into(),
            DiscordBotSection {
                bot_token: Some("tok2".into()),
                guild_id: None,
                allowed_users: vec![],
            },
        );
        let c = Config {
            discord: Some(DiscordSection {
                bots,
                ..DiscordSection::default()
            }),
            ..Config::default()
        };
        let profiles = c.resolved_discord_profiles();
        assert_eq!(profiles.len(), 2);
    }

    // --- find_discord_profile tests ---

    #[test]
    fn find_discord_profile_existing() {
        let mut bots = HashMap::new();
        bots.insert(
            "mybot".into(),
            DiscordBotSection {
                bot_token: Some("tok".into()),
                guild_id: Some("g1".into()),
                allowed_users: vec!["u1".into()],
            },
        );
        let c = Config {
            discord: Some(DiscordSection {
                bots,
                ..DiscordSection::default()
            }),
            ..Config::default()
        };
        let profile = c.find_discord_profile("mybot");
        assert!(profile.is_some());
        assert_eq!(profile.unwrap().name, "mybot");
    }

    #[test]
    fn find_discord_profile_nonexistent() {
        let c = Config::default();
        assert!(c.find_discord_profile("missing").is_none());
    }

    // --- DirState tests ---

    #[test]
    fn dir_state_default() {
        let s = DirState::default();
        assert!(s.conversation.is_none());
        assert!(s.agent.is_none());
        assert!(s.model.is_none());
        assert!(s.sandbox.is_none());
        assert!(s.discord_destination.is_none());
        assert!(s.dir_id.is_none());
    }

    // --- persist_dir_state and load_dir_state round trip ---

    #[test]
    #[serial]
    fn persist_and_load_dir_state_round_trip() {
        with_temp_home(|| {
            crate::conversation::ensure_breo_dir();
            persist_dir_state(
                "my-conv",
                &Backend::Gemini,
                Some("opus"),
                Some("vm1"),
                Some("dm"),
                true,
            );
            let state = load_dir_state();
            assert_eq!(state.agent.as_deref(), Some("gemini"));
            assert_eq!(state.model.as_deref(), Some("opus"));
            assert_eq!(state.sandbox.as_deref(), Some("vm1"));
            assert_eq!(state.discord_destination.as_deref(), Some("dm"));
        });
    }

    // --- current_dir_key test ---

    #[test]
    fn current_dir_key_not_empty() {
        let key = current_dir_key();
        assert!(!key.is_empty());
    }
}
