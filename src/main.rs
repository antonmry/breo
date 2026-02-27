mod claws;
mod config;
mod conversation;
mod loop_cmd;
mod sandbox;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::engine::{ArgValueCandidates, ArgValueCompleter, PathCompleter};
use clap_complete::env::CompleteEnv;
use std::io::{self, IsTerminal, Read as _};
use std::path::PathBuf;

use crate::claws::{DiscordDestination, cmd_claws, cmd_claws_list};
use crate::config::{
    Backend, ShellType, cmd_setup, cmd_status, list_conversations, list_discord_bots, list_models,
    load_config, load_dir_state, persist_dir_state,
};
use crate::conversation::{cmd_compact, cmd_list, cmd_new, cmd_pick, cmd_rename, cmd_send};
use crate::loop_cmd::cmd_loop;

#[derive(Parser)]
#[command(
    name = "breo",
    version,
    about = "Chat with an LLM, keeping conversation in a markdown file"
)]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    message: Option<String>,

    #[arg(short, long, add = ArgValueCandidates::new(list_conversations))]
    conversation: Option<String>,

    #[arg(short, long, add = ArgValueCandidates::new(list_models))]
    model: Option<String>,

    #[arg(short, long, value_enum)]
    agent: Option<Backend>,

    #[arg(short, long, num_args = 1.., add = ArgValueCompleter::new(PathCompleter::file()))]
    files: Vec<PathBuf>,

    #[arg(short, long)]
    sandbox: Option<String>,

    #[arg(long)]
    no_sandbox: bool,

    #[arg(long)]
    no_push: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    New {
        name: String,
    },
    List,
    Pick,
    Setup {
        #[arg(value_enum)]
        shell: ShellType,
    },
    Status,
    Rename {
        #[arg(add = ArgValueCandidates::new(list_conversations))]
        old_name: String,
        new_name: String,
    },
    Compact {
        #[arg(add = ArgValueCandidates::new(list_conversations))]
        name: Option<String>,
    },
    Loop {
        plan: PathBuf,
        verification: PathBuf,

        #[arg(short, long, value_enum)]
        agent: Option<Backend>,

        #[arg(long, value_enum)]
        review_agent: Option<Backend>,

        #[arg(long, add = ArgValueCandidates::new(list_models))]
        review_model: Option<String>,

        #[arg(short, long, add = ArgValueCandidates::new(list_conversations))]
        conversation: Option<String>,

        #[arg(short, long, num_args = 1.., add = ArgValueCompleter::new(PathCompleter::file()))]
        files: Vec<PathBuf>,

        #[arg(short, long)]
        sandbox: Option<String>,

        #[arg(long)]
        no_sandbox: bool,
    },
    #[command(long_about = "\
Start the Discord bot bridge for the current directory.

The bot responds to DMs and @mentions in channels. Messages are routed
through the same LLM backend as `breo send`, with full conversation
persistence. Use --listen-all to process all messages in the destination
channel without requiring @mentions.

Bot commands (send as a Discord message):
  !switch <name>    Switch to a different conversation
  !new <name>       Create a new conversation and switch to it
  !list             List all conversations
  !status           Show bot name, directory, conversation, agent, model, sandbox, destination
  !agent <backend>  Change the LLM backend (claude, codex, gemini, ...)
  !model <name>     Change the model
  !destination <channel_id|dm>  Change where responses are delivered
  !receive-all [on|off]  Toggle receiving all messages in channel (omit arg to toggle)
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
        #[arg(add = ArgValueCandidates::new(list_discord_bots))]
        bot: String,

        #[arg(short, long, value_enum)]
        agent: Option<Backend>,

        #[arg(short, long, add = ArgValueCandidates::new(list_models))]
        model: Option<String>,

        #[arg(short, long)]
        sandbox: Option<String>,

        #[arg(long)]
        guild_id: Option<String>,

        #[arg(short = 'd', long)]
        destination: Option<String>,

        /// Receive all messages in the destination channel (not just @mentions)
        #[arg(long)]
        receive_all: bool,
    },
}

fn resolve_backend(
    cli_agent: Option<&Backend>,
    dir_state_agent: Option<&str>,
    config_agent: &str,
) -> Backend {
    if let Some(be) = cli_agent {
        return be.clone();
    }
    if let Some(a) = dir_state_agent {
        match a {
            "codex" => return Backend::Codex,
            "gemini" => return Backend::Gemini,
            "claude" => return Backend::Claude,
            _ => {}
        }
    }
    match config_agent {
        "codex" => Backend::Codex,
        "gemini" => Backend::Gemini,
        _ => Backend::Claude,
    }
}

fn resolve_push(no_push: bool, config_push: bool) -> bool {
    if no_push { false } else { config_push }
}

fn resolve_model(cli_model: Option<String>, dir_state_model: Option<String>) -> Option<String> {
    cli_model.or(dir_state_model)
}

fn resolve_destination(
    cli_destination: Option<&str>,
    dir_state_destination: Option<&str>,
) -> DiscordDestination {
    cli_destination
        .and_then(DiscordDestination::parse)
        .or_else(|| dir_state_destination.and_then(DiscordDestination::parse))
        .unwrap_or(DiscordDestination::Dm)
}

fn resolve_sandbox(
    no_sandbox: bool,
    cli_sandbox: Option<String>,
    dir_state_sandbox: Option<&str>,
    config_sandbox: bool,
    config_sandbox_name: &str,
) -> Option<String> {
    if no_sandbox {
        None
    } else if let Some(name) = cli_sandbox {
        Some(name)
    } else if let Some(name) = dir_state_sandbox {
        Some(name.to_string())
    } else if config_sandbox {
        Some(config_sandbox_name.to_string())
    } else {
        None
    }
}

/// Resolves loop-specific sandbox: loop_no_sandbox overrides, then loop_sandbox, then default.
fn resolve_loop_sandbox(
    loop_no_sandbox: bool,
    loop_sandbox: Option<String>,
    default_sandbox: Option<String>,
) -> Option<String> {
    if loop_no_sandbox {
        None
    } else if let Some(name) = loop_sandbox {
        Some(name)
    } else {
        default_sandbox
    }
}

/// Validates a Discord bot profile: ensures it exists and has allowed users.
fn validate_claws_profile(
    bot: &str,
    config: &config::Config,
) -> Result<config::DiscordBotProfile, String> {
    let profile = config.find_discord_profile(bot).ok_or_else(|| {
        format!(
            "Discord bot profile '{bot}' was not found.\n\
             Use `breo claws list` to view configured bot profiles."
        )
    })?;
    if profile.allowed_users.is_empty() {
        return Err(format!(
            "Profile '{bot}' has no allowed users.\n\
             Add `allowed_users = [\"...\"]` under [discord.bots.{bot}] in config.toml."
        ));
    }
    Ok(profile)
}

fn main() {
    CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();
    let config = load_config();
    let dir_state = load_dir_state();

    let backend = resolve_backend(
        cli.agent.as_ref(),
        dir_state.agent.as_deref(),
        &config.agent,
    );

    let sandbox_name = resolve_sandbox(
        cli.no_sandbox,
        cli.sandbox,
        dir_state.sandbox.as_deref(),
        config.sandbox,
        &config.sandbox_name,
    );
    let sandbox = sandbox_name.as_deref();

    let push = resolve_push(cli.no_push, config.push);

    let resolved_model: Option<String> = resolve_model(cli.model.clone(), dir_state.model.clone());

    let save_after_send = |conversation: &str| {
        persist_dir_state(
            conversation,
            &backend,
            resolved_model.as_deref(),
            sandbox,
            dir_state.discord_destination.as_deref(),
            dir_state.receive_all,
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
                receive_all,
            }),
        ) => {
            if bot == "list" {
                cmd_claws_list(&config);
                return;
            }

            let profile = match validate_claws_profile(&bot, &config) {
                Ok(p) => p,
                Err(msg) => {
                    eprintln!("{msg}");
                    std::process::exit(1);
                }
            };

            let claws_backend = claws_agent.unwrap_or(backend);
            let claws_resolved_model = claws_model.or(resolved_model);
            let claws_sandbox_name = claws_sandbox.or(sandbox_name);
            let claws_guild = claws_guild_id.or(profile.guild_id);
            let claws_destination = resolve_destination(
                claws_destination.as_deref(),
                dir_state.discord_destination.as_deref(),
            );
            let claws_receive_all = if receive_all {
                true
            } else {
                dir_state.receive_all.unwrap_or(false)
            };

            cmd_claws(
                &profile.name,
                &profile.bot_token,
                claws_guild,
                profile.allowed_users,
                claws_backend,
                claws_resolved_model,
                claws_sandbox_name,
                claws_destination,
                claws_receive_all,
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
            let loop_sandbox_name =
                resolve_loop_sandbox(loop_no_sandbox, loop_sandbox, sandbox_name.clone());
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
            Cli::command().print_help().expect("print help");
            println!();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn resolve_backend_cli_takes_priority() {
        let be = resolve_backend(Some(&Backend::Codex), Some("gemini"), "claude");
        assert!(matches!(be, Backend::Codex));
    }

    #[test]
    fn resolve_backend_dir_state_fallback() {
        let be = resolve_backend(None, Some("gemini"), "claude");
        assert!(matches!(be, Backend::Gemini));
    }

    #[test]
    fn resolve_backend_config_fallback() {
        let be = resolve_backend(None, None, "codex");
        assert!(matches!(be, Backend::Codex));
    }

    #[test]
    fn resolve_backend_default_is_claude() {
        let be = resolve_backend(None, None, "unknown");
        assert!(matches!(be, Backend::Claude));
    }

    #[test]
    fn resolve_backend_unknown_dir_state_falls_through() {
        let be = resolve_backend(None, Some("unknown-agent"), "gemini");
        assert!(matches!(be, Backend::Gemini));
    }

    #[test]
    fn resolve_sandbox_no_sandbox_flag() {
        let s = resolve_sandbox(true, Some("vm".into()), Some("vm2"), true, "default");
        assert!(s.is_none());
    }

    #[test]
    fn resolve_sandbox_cli_takes_priority() {
        let s = resolve_sandbox(false, Some("myvm".into()), Some("other"), true, "default");
        assert_eq!(s.as_deref(), Some("myvm"));
    }

    #[test]
    fn resolve_sandbox_dir_state_fallback() {
        let s = resolve_sandbox(false, None, Some("saved"), true, "default");
        assert_eq!(s.as_deref(), Some("saved"));
    }

    #[test]
    fn resolve_sandbox_config_fallback() {
        let s = resolve_sandbox(false, None, None, true, "default");
        assert_eq!(s.as_deref(), Some("default"));
    }

    #[test]
    fn resolve_sandbox_none_when_disabled() {
        let s = resolve_sandbox(false, None, None, false, "default");
        assert!(s.is_none());
    }

    #[test]
    fn cli_parse_version() {
        let result = Cli::try_parse_from(["breo", "--version"]);
        // --version causes early exit, which clap reports as an error
        assert!(result.is_err());
    }

    #[test]
    fn cli_parse_help() {
        let result = Cli::try_parse_from(["breo", "--help"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_parse_message() {
        let cli = Cli::try_parse_from(["breo", "hello world"]).expect("parse");
        assert_eq!(cli.message.as_deref(), Some("hello world"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_parse_new_subcommand() {
        let cli = Cli::try_parse_from(["breo", "new", "my-conv"]).expect("parse");
        assert!(matches!(cli.command, Some(Commands::New { .. })));
    }

    #[test]
    fn cli_parse_list_subcommand() {
        let cli = Cli::try_parse_from(["breo", "list"]).expect("parse");
        assert!(matches!(cli.command, Some(Commands::List)));
    }

    #[test]
    fn cli_parse_status_subcommand() {
        let cli = Cli::try_parse_from(["breo", "status"]).expect("parse");
        assert!(matches!(cli.command, Some(Commands::Status)));
    }

    #[test]
    fn cli_parse_with_agent_flag() {
        let cli = Cli::try_parse_from(["breo", "-a", "gemini", "test msg"]).expect("parse");
        assert!(matches!(cli.agent, Some(Backend::Gemini)));
        assert_eq!(cli.message.as_deref(), Some("test msg"));
    }

    #[test]
    fn cli_parse_with_model_flag() {
        let cli = Cli::try_parse_from(["breo", "-m", "opus", "test msg"]).expect("parse");
        assert_eq!(cli.model.as_deref(), Some("opus"));
    }

    #[test]
    fn cli_parse_with_no_sandbox() {
        let cli = Cli::try_parse_from(["breo", "--no-sandbox", "msg"]).expect("parse");
        assert!(cli.no_sandbox);
    }

    #[test]
    fn cli_parse_with_no_push() {
        let cli = Cli::try_parse_from(["breo", "--no-push", "msg"]).expect("parse");
        assert!(cli.no_push);
    }

    #[test]
    fn cli_parse_rename_subcommand() {
        let cli = Cli::try_parse_from(["breo", "rename", "old", "new"]).expect("parse");
        assert!(matches!(cli.command, Some(Commands::Rename { .. })));
    }

    #[test]
    fn cli_parse_compact_subcommand() {
        let cli = Cli::try_parse_from(["breo", "compact"]).expect("parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Compact { name: None })
        ));
    }

    #[test]
    fn cli_parse_compact_with_name() {
        let cli = Cli::try_parse_from(["breo", "compact", "my-conv"]).expect("parse");
        match cli.command {
            Some(Commands::Compact { name }) => assert_eq!(name.as_deref(), Some("my-conv")),
            _ => panic!("expected Compact"),
        }
    }

    #[test]
    fn cli_parse_setup_bash() {
        let cli = Cli::try_parse_from(["breo", "setup", "bash"]).expect("parse");
        assert!(matches!(cli.command, Some(Commands::Setup { .. })));
    }

    #[test]
    fn cli_parse_claws_subcommand() {
        let cli = Cli::try_parse_from(["breo", "claws", "mybot"]).expect("parse");
        match cli.command {
            Some(Commands::Claws { bot, .. }) => assert_eq!(bot, "mybot"),
            _ => panic!("expected Claws"),
        }
    }

    #[test]
    fn cli_parse_claws_with_flags() {
        let cli = Cli::try_parse_from([
            "breo", "claws", "mybot", "-a", "gemini", "-m", "opus", "-d", "dm",
        ])
        .expect("parse");
        match cli.command {
            Some(Commands::Claws {
                bot,
                agent,
                model,
                destination,
                ..
            }) => {
                assert_eq!(bot, "mybot");
                assert!(matches!(agent, Some(Backend::Gemini)));
                assert_eq!(model.as_deref(), Some("opus"));
                assert_eq!(destination.as_deref(), Some("dm"));
            }
            _ => panic!("expected Claws"),
        }
    }

    #[test]
    fn cli_parse_loop_subcommand() {
        let cli = Cli::try_parse_from(["breo", "loop", "PLAN.md", "VERIFY.md"]).expect("parse");
        assert!(matches!(cli.command, Some(Commands::Loop { .. })));
    }

    #[test]
    fn cli_parse_loop_with_all_flags() {
        let cli = Cli::try_parse_from([
            "breo",
            "loop",
            "PLAN.md",
            "VERIFY.md",
            "-a",
            "gemini",
            "--review-agent",
            "claude",
            "--review-model",
            "opus",
            "-c",
            "my-conv",
            "-s",
            "vm1",
        ])
        .expect("parse");
        match cli.command {
            Some(Commands::Loop {
                plan,
                verification,
                agent,
                review_agent,
                review_model,
                conversation,
                sandbox,
                ..
            }) => {
                assert_eq!(plan.to_string_lossy(), "PLAN.md");
                assert_eq!(verification.to_string_lossy(), "VERIFY.md");
                assert!(matches!(agent, Some(Backend::Gemini)));
                assert!(matches!(review_agent, Some(Backend::Claude)));
                assert_eq!(review_model.as_deref(), Some("opus"));
                assert_eq!(conversation.as_deref(), Some("my-conv"));
                assert_eq!(sandbox.as_deref(), Some("vm1"));
            }
            _ => panic!("expected Loop"),
        }
    }

    #[test]
    fn cli_parse_loop_no_sandbox() {
        let cli = Cli::try_parse_from(["breo", "loop", "PLAN.md", "VERIFY.md", "--no-sandbox"])
            .expect("parse");
        match cli.command {
            Some(Commands::Loop { no_sandbox, .. }) => assert!(no_sandbox),
            _ => panic!("expected Loop"),
        }
    }

    #[test]
    fn cli_parse_message_with_conversation() {
        let cli = Cli::try_parse_from(["breo", "-c", "my-conv", "hello"]).expect("parse");
        assert_eq!(cli.conversation.as_deref(), Some("my-conv"));
        assert_eq!(cli.message.as_deref(), Some("hello"));
    }

    #[test]
    fn cli_parse_message_with_files() {
        // -f uses num_args=1.. so it consumes all following args as files
        let cli = Cli::try_parse_from(["breo", "hello", "-f", "a.txt", "b.rs"]).expect("parse");
        assert_eq!(cli.files.len(), 2);
        assert_eq!(cli.message.as_deref(), Some("hello"));
    }

    #[test]
    fn cli_parse_message_with_sandbox() {
        let cli = Cli::try_parse_from(["breo", "-s", "default", "hello"]).expect("parse");
        assert_eq!(cli.sandbox.as_deref(), Some("default"));
    }

    #[test]
    fn resolve_backend_claude_dir_state() {
        let be = resolve_backend(None, Some("claude"), "gemini");
        assert!(matches!(be, Backend::Claude));
    }

    #[test]
    fn resolve_backend_codex_dir_state() {
        let be = resolve_backend(None, Some("codex"), "claude");
        assert!(matches!(be, Backend::Codex));
    }

    #[test]
    fn resolve_backend_gemini_config() {
        let be = resolve_backend(None, None, "gemini");
        assert!(matches!(be, Backend::Gemini));
    }

    #[test]
    fn resolve_backend_codex_config() {
        let be = resolve_backend(None, None, "codex");
        assert!(matches!(be, Backend::Codex));
    }

    #[test]
    fn resolve_sandbox_empty_string() {
        let s = resolve_sandbox(false, Some("".into()), None, false, "default");
        assert_eq!(s.as_deref(), Some(""));
    }

    #[test]
    fn resolve_sandbox_all_none() {
        let s = resolve_sandbox(false, None, None, false, "");
        assert!(s.is_none());
    }

    #[test]
    fn cli_parse_claws_with_destination() {
        let cli = Cli::try_parse_from(["breo", "claws", "mybot", "-d", "123456"]).expect("parse");
        match cli.command {
            Some(Commands::Claws {
                bot, destination, ..
            }) => {
                assert_eq!(bot, "mybot");
                assert_eq!(destination.as_deref(), Some("123456"));
            }
            _ => panic!("expected Claws"),
        }
    }

    #[test]
    fn cli_parse_claws_with_guild_id() {
        let cli =
            Cli::try_parse_from(["breo", "claws", "mybot", "--guild-id", "789"]).expect("parse");
        match cli.command {
            Some(Commands::Claws { guild_id, .. }) => {
                assert_eq!(guild_id.as_deref(), Some("789"));
            }
            _ => panic!("expected Claws"),
        }
    }

    #[test]
    fn cli_parse_claws_with_sandbox() {
        let cli = Cli::try_parse_from(["breo", "claws", "mybot", "-s", "vm1"]).expect("parse");
        match cli.command {
            Some(Commands::Claws { sandbox, .. }) => {
                assert_eq!(sandbox.as_deref(), Some("vm1"));
            }
            _ => panic!("expected Claws"),
        }
    }

    #[test]
    fn cli_parse_claws_with_receive_all() {
        let cli = Cli::try_parse_from(["breo", "claws", "mybot", "--receive-all"]).expect("parse");
        match cli.command {
            Some(Commands::Claws { receive_all, .. }) => {
                assert!(receive_all);
            }
            _ => panic!("expected Claws"),
        }
    }

    #[test]
    fn cli_parse_rename_fields() {
        let cli = Cli::try_parse_from(["breo", "rename", "old-conv", "new-conv"]).expect("parse");
        match cli.command {
            Some(Commands::Rename { old_name, new_name }) => {
                assert_eq!(old_name, "old-conv");
                assert_eq!(new_name, "new-conv");
            }
            _ => panic!("expected Rename"),
        }
    }

    #[test]
    fn cli_parse_setup_zsh() {
        let cli = Cli::try_parse_from(["breo", "setup", "zsh"]).expect("parse");
        match cli.command {
            Some(Commands::Setup { shell }) => assert!(matches!(shell, ShellType::Zsh)),
            _ => panic!("expected Setup"),
        }
    }

    #[test]
    fn cli_parse_setup_fish() {
        let cli = Cli::try_parse_from(["breo", "setup", "fish"]).expect("parse");
        match cli.command {
            Some(Commands::Setup { shell }) => assert!(matches!(shell, ShellType::Fish)),
            _ => panic!("expected Setup"),
        }
    }

    #[test]
    fn cli_parse_no_args_no_subcommand() {
        let cli = Cli::try_parse_from(["breo"]).expect("parse");
        assert!(cli.message.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn resolve_push_no_push_flag() {
        assert!(!resolve_push(true, true));
        assert!(!resolve_push(true, false));
    }

    #[test]
    fn resolve_push_config_default() {
        assert!(resolve_push(false, true));
        assert!(!resolve_push(false, false));
    }

    #[test]
    fn resolve_model_cli_takes_priority() {
        let m = resolve_model(Some("opus".into()), Some("sonnet".into()));
        assert_eq!(m.as_deref(), Some("opus"));
    }

    #[test]
    fn resolve_model_dir_state_fallback() {
        let m = resolve_model(None, Some("sonnet".into()));
        assert_eq!(m.as_deref(), Some("sonnet"));
    }

    #[test]
    fn resolve_model_none() {
        let m = resolve_model(None, None);
        assert!(m.is_none());
    }

    #[test]
    fn resolve_destination_cli_dm() {
        let d = resolve_destination(Some("dm"), None);
        assert!(matches!(d, DiscordDestination::Dm));
    }

    #[test]
    fn resolve_destination_cli_channel() {
        let d = resolve_destination(Some("123456"), None);
        assert!(matches!(d, DiscordDestination::Channel(_)));
    }

    #[test]
    fn resolve_destination_dir_state_fallback() {
        let d = resolve_destination(None, Some("789"));
        assert!(matches!(d, DiscordDestination::Channel(_)));
    }

    #[test]
    fn resolve_destination_default_dm() {
        let d = resolve_destination(None, None);
        assert!(matches!(d, DiscordDestination::Dm));
    }

    #[test]
    fn resolve_destination_cli_overrides_dir_state() {
        let d = resolve_destination(Some("dm"), Some("123"));
        assert!(matches!(d, DiscordDestination::Dm));
    }

    // --- resolve_loop_sandbox tests ---

    #[test]
    fn resolve_loop_sandbox_no_sandbox_flag() {
        let s = resolve_loop_sandbox(true, Some("vm".into()), Some("default".into()));
        assert!(s.is_none());
    }

    #[test]
    fn resolve_loop_sandbox_cli_takes_priority() {
        let s = resolve_loop_sandbox(false, Some("myvm".into()), Some("default".into()));
        assert_eq!(s.as_deref(), Some("myvm"));
    }

    #[test]
    fn resolve_loop_sandbox_default_fallback() {
        let s = resolve_loop_sandbox(false, None, Some("default".into()));
        assert_eq!(s.as_deref(), Some("default"));
    }

    #[test]
    fn resolve_loop_sandbox_all_none() {
        let s = resolve_loop_sandbox(false, None, None);
        assert!(s.is_none());
    }

    // --- validate_claws_profile tests ---

    #[test]
    fn validate_claws_profile_not_found() {
        let config = config::Config::default();
        let result = validate_claws_profile("missing-bot", &config);
        match result {
            Err(err) => assert!(err.contains("not found")),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn validate_claws_profile_no_allowed_users() {
        let mut bots = std::collections::HashMap::new();
        bots.insert(
            "empty-bot".into(),
            config::DiscordBotSection {
                bot_token: Some("tok".into()),
                guild_id: None,
                allowed_users: vec![],
            },
        );
        let config = config::Config {
            discord: Some(config::DiscordSection {
                bots,
                ..config::DiscordSection::default()
            }),
            ..config::Config::default()
        };
        let result = validate_claws_profile("empty-bot", &config);
        match result {
            Err(err) => assert!(err.contains("no allowed users")),
            Ok(_) => panic!("expected error"),
        }
    }

    #[test]
    fn validate_claws_profile_valid() {
        let mut bots = std::collections::HashMap::new();
        bots.insert(
            "good-bot".into(),
            config::DiscordBotSection {
                bot_token: Some("tok".into()),
                guild_id: Some("g1".into()),
                allowed_users: vec!["u1".into()],
            },
        );
        let config = config::Config {
            discord: Some(config::DiscordSection {
                bots,
                ..config::DiscordSection::default()
            }),
            ..config::Config::default()
        };
        let result = validate_claws_profile("good-bot", &config);
        assert!(result.is_ok());
        let profile = result.unwrap();
        assert_eq!(profile.name, "good-bot");
        assert_eq!(profile.allowed_users, vec!["u1"]);
    }

    #[test]
    fn validate_claws_profile_error_messages_mention_bot_name() {
        let config = config::Config::default();
        match validate_claws_profile("my-bot", &config) {
            Err(err) => assert!(err.contains("my-bot")),
            Ok(_) => panic!("expected error"),
        }
    }

    // --- more resolve_backend tests ---

    #[test]
    fn resolve_backend_all_dir_state_variants() {
        for (input, expected) in [
            ("claude", Backend::Claude),
            ("codex", Backend::Codex),
            ("gemini", Backend::Gemini),
        ] {
            let be = resolve_backend(None, Some(input), "default");
            assert!(
                std::mem::discriminant(&be) == std::mem::discriminant(&expected),
                "failed for {input}"
            );
        }
    }

    #[test]
    fn resolve_backend_all_config_variants() {
        for (input, expected) in [
            ("claude", Backend::Claude),
            ("codex", Backend::Codex),
            ("gemini", Backend::Gemini),
        ] {
            let be = resolve_backend(None, None, input);
            assert!(
                std::mem::discriminant(&be) == std::mem::discriminant(&expected),
                "failed for {input}"
            );
        }
    }

    #[test]
    fn resolve_backend_cli_overrides_everything() {
        for cli_backend in [Backend::Claude, Backend::Codex, Backend::Gemini] {
            let be = resolve_backend(Some(&cli_backend), Some("gemini"), "codex");
            assert!(std::mem::discriminant(&be) == std::mem::discriminant(&cli_backend),);
        }
    }

    // --- more resolve_sandbox tests ---

    #[test]
    fn resolve_sandbox_priority_chain() {
        // no_sandbox=true beats everything
        assert!(resolve_sandbox(true, Some("a".into()), Some("b"), true, "c").is_none());
        // CLI beats dir_state and config
        assert_eq!(
            resolve_sandbox(false, Some("cli".into()), Some("dir"), true, "cfg"),
            Some("cli".into())
        );
        // dir_state beats config
        assert_eq!(
            resolve_sandbox(false, None, Some("dir"), true, "cfg"),
            Some("dir".into())
        );
        // config is last resort
        assert_eq!(
            resolve_sandbox(false, None, None, true, "cfg"),
            Some("cfg".into())
        );
    }

    // --- more resolve_push tests ---

    #[test]
    fn resolve_push_truth_table() {
        assert!(!resolve_push(true, true));
        assert!(!resolve_push(true, false));
        assert!(resolve_push(false, true));
        assert!(!resolve_push(false, false));
    }

    // --- more resolve_model tests ---

    #[test]
    fn resolve_model_cli_with_empty_string() {
        let m = resolve_model(Some("".into()), Some("fallback".into()));
        assert_eq!(m.as_deref(), Some(""));
    }

    // --- more resolve_destination tests ---

    #[test]
    fn resolve_destination_all_channel_formats() {
        for input in ["123", "999888777666", "1"] {
            let d = resolve_destination(Some(input), None);
            assert!(
                matches!(d, DiscordDestination::Channel(_)),
                "expected Channel for {input}"
            );
        }
    }

    // --- more resolve_loop_sandbox tests ---

    #[test]
    fn resolve_loop_sandbox_priority_chain() {
        assert!(resolve_loop_sandbox(true, Some("a".into()), Some("b".into())).is_none());
        assert_eq!(
            resolve_loop_sandbox(false, Some("loop".into()), Some("default".into())),
            Some("loop".into())
        );
        assert_eq!(
            resolve_loop_sandbox(false, None, Some("default".into())),
            Some("default".into())
        );
        assert!(resolve_loop_sandbox(false, None, None).is_none());
    }

    // --- more validate_claws_profile tests ---

    #[test]
    fn validate_claws_profile_multiple_allowed_users() {
        let mut bots = std::collections::HashMap::new();
        bots.insert(
            "multi-user-bot".into(),
            config::DiscordBotSection {
                bot_token: Some("tok".into()),
                guild_id: None,
                allowed_users: vec!["u1".into(), "u2".into(), "u3".into()],
            },
        );
        let config = config::Config {
            discord: Some(config::DiscordSection {
                bots,
                ..config::DiscordSection::default()
            }),
            ..config::Config::default()
        };
        let result = validate_claws_profile("multi-user-bot", &config);
        assert!(result.is_ok());
        let profile = result.unwrap();
        assert_eq!(profile.allowed_users.len(), 3);
    }

    #[test]
    fn validate_claws_profile_no_guild_id() {
        let mut bots = std::collections::HashMap::new();
        bots.insert(
            "no-guild".into(),
            config::DiscordBotSection {
                bot_token: Some("tok".into()),
                guild_id: None,
                allowed_users: vec!["u1".into()],
            },
        );
        let config = config::Config {
            discord: Some(config::DiscordSection {
                bots,
                ..config::DiscordSection::default()
            }),
            ..config::Config::default()
        };
        let profile = validate_claws_profile("no-guild", &config).unwrap();
        assert!(profile.guild_id.is_none());
    }

    #[test]
    fn validate_claws_profile_with_guild_id() {
        let mut bots = std::collections::HashMap::new();
        bots.insert(
            "guild-bot".into(),
            config::DiscordBotSection {
                bot_token: Some("tok".into()),
                guild_id: Some("g123".into()),
                allowed_users: vec!["u1".into()],
            },
        );
        let config = config::Config {
            discord: Some(config::DiscordSection {
                bots,
                ..config::DiscordSection::default()
            }),
            ..config::Config::default()
        };
        let profile = validate_claws_profile("guild-bot", &config).unwrap();
        assert_eq!(profile.guild_id.as_deref(), Some("g123"));
    }

    // --- CLI parse additional tests ---

    #[test]
    fn cli_parse_pick_subcommand() {
        let cli = Cli::try_parse_from(["breo", "pick"]).expect("parse");
        assert!(matches!(cli.command, Some(Commands::Pick)));
    }

    #[test]
    fn cli_parse_invalid_subcommand() {
        let result = Cli::try_parse_from(["breo", "nonexistent-subcommand"]);
        // "nonexistent-subcommand" is treated as a message, not an error
        assert!(result.is_ok());
        let cli = result.unwrap();
        assert_eq!(cli.message.as_deref(), Some("nonexistent-subcommand"));
    }

    #[test]
    fn cli_parse_loop_with_files() {
        let cli = Cli::try_parse_from(["breo", "loop", "P.md", "V.md", "-f", "a.rs", "b.rs"])
            .expect("parse");
        match cli.command {
            Some(Commands::Loop { files, .. }) => assert_eq!(files.len(), 2),
            _ => panic!("expected Loop"),
        }
    }

    #[test]
    fn cli_parse_claws_no_optional_flags() {
        let cli = Cli::try_parse_from(["breo", "claws", "minimal-bot"]).expect("parse");
        match cli.command {
            Some(Commands::Claws {
                bot,
                agent,
                model,
                sandbox,
                guild_id,
                destination,
                receive_all,
            }) => {
                assert_eq!(bot, "minimal-bot");
                assert!(agent.is_none());
                assert!(model.is_none());
                assert!(sandbox.is_none());
                assert!(guild_id.is_none());
                assert!(destination.is_none());
                assert!(!receive_all);
            }
            _ => panic!("expected Claws"),
        }
    }

    #[test]
    fn cli_parse_message_with_all_flags() {
        let cli = Cli::try_parse_from([
            "breo",
            "-c",
            "my-conv",
            "-m",
            "opus",
            "-a",
            "claude",
            "-s",
            "vm1",
            "--no-push",
            "hello",
        ])
        .expect("parse");
        assert_eq!(cli.message.as_deref(), Some("hello"));
        assert_eq!(cli.conversation.as_deref(), Some("my-conv"));
        assert_eq!(cli.model.as_deref(), Some("opus"));
        assert!(matches!(cli.agent, Some(Backend::Claude)));
        assert_eq!(cli.sandbox.as_deref(), Some("vm1"));
        assert!(cli.no_push);
    }
}
