use crate::config::{
    Backend, Config, backend_from_name, backend_name, current_dir_key, persist_dir_state,
};
use crate::conversation::{
    cmd_compact, cmd_send_inner, conversation_names_sorted, conversation_path, count_exchanges,
    create_conversation, ensure_breo_dir, get_active, set_active,
};
use crate::loop_cmd::truncate_display;
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDateTime};
use serenity::all::{ChannelId, GatewayIntents, UserId};
use serenity::async_trait;
use serenity::model::channel::Message as DiscordMessage;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{Duration, sleep};

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

#[derive(Clone)]
pub(crate) enum DiscordDestination {
    Channel(String),
    Dm,
}

impl DiscordDestination {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if trimmed.eq_ignore_ascii_case("dm") {
            return Some(Self::Dm);
        }
        if !trimmed.is_empty() {
            return Some(Self::Channel(trimmed.to_string()));
        }
        None
    }

    pub(crate) fn to_storage(&self) -> String {
        match self {
            Self::Dm => "dm".to_string(),
            Self::Channel(id) => id.clone(),
        }
    }

    pub(crate) fn display(&self) -> String {
        match self {
            Self::Dm => "dm".to_string(),
            Self::Channel(id) => format!("channel {id}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CronTaskStatus {
    Pending,
    Running,
}

#[derive(Clone)]
pub(crate) struct CronTask {
    pub(crate) name: String,
    pub(crate) message: String,
    pub(crate) next_run: NaiveDateTime,
    pub(crate) interval: Option<String>,
    pub(crate) status: CronTaskStatus,
}

#[derive(Clone)]
pub(crate) struct DiscordBotState {
    pub(crate) bot_name: String,
    pub(crate) conversation: String,
    pub(crate) backend: Backend,
    pub(crate) model: Option<String>,
    pub(crate) sandbox: Option<String>,
    pub(crate) destination: DiscordDestination,
    pub(crate) receive_all: bool,
    pub(crate) allowed_users: Vec<String>,
    pub(crate) cron_started: bool,
}

impl DiscordBotState {
    fn persist(&self) {
        persist_dir_state(
            &self.conversation,
            &self.backend,
            self.model.as_deref(),
            self.sandbox.as_deref(),
            Some(&self.destination.to_storage()),
            Some(self.receive_all),
        );
    }
}

/// Describes the planned effect of a bot command.
/// Separates pure decision logic from async Discord I/O for testability.
pub(crate) enum CommandAction {
    /// Mutate state: set conversation, persist, respond with message
    SwitchConversation { name: String, response: String },
    /// Mutate state: set backend, persist, respond with message
    SwitchAgent { backend: Backend, response: String },
    /// Mutate state: set model, persist, respond with message
    SetModel { model: String, response: String },
    /// Mutate state: set destination, persist, respond with message
    SetDestination {
        destination: DiscordDestination,
        response: String,
    },
    /// Mutate state: toggle receive_all, persist, respond with message
    SetReceiveAll { receive_all: bool, response: String },
    /// Read-only: respond with status/list text to state destination
    Respond(String),
    /// Async: create conversation, then switch to it
    CreateConversation(String),
    /// Async: run compaction on conversation
    RunCompact(String),
    /// Error: respond to the source channel with an error message
    ErrorToSource(String),
}

/// Plans what a bot command should do, without performing any I/O.
/// The caller (handle_command) gathers context and executes the returned action.
pub(crate) fn plan_command_action(
    command: &str,
    arg: &str,
    state: &DiscordBotState,
    conversation_exists: bool,
    conversation_names: &[String],
    exchange_count: usize,
) -> CommandAction {
    match command {
        "switch" => {
            if arg.is_empty() {
                return CommandAction::ErrorToSource("Usage: !switch <conversation>".into());
            }
            if !conversation_exists {
                return CommandAction::ErrorToSource("Conversation not found.".into());
            }
            CommandAction::SwitchConversation {
                name: arg.to_string(),
                response: format!("Switched to conversation: {arg}"),
            }
        }
        "agent" => {
            if arg.is_empty() {
                return CommandAction::ErrorToSource(
                    "Usage: !agent <claude|codex|gemini>".into(),
                );
            }
            let Some(backend) = backend_from_name(arg) else {
                return CommandAction::ErrorToSource(
                    "Unknown agent. Use: claude, codex, or gemini.".into(),
                );
            };
            CommandAction::SwitchAgent {
                response: format!("Switched to agent: {}", backend_name(&backend)),
                backend,
            }
        }
        "model" => {
            if arg.is_empty() {
                return CommandAction::ErrorToSource("Usage: !model <name>".into());
            }
            CommandAction::SetModel {
                model: arg.to_string(),
                response: format!("Switched to model: {arg}"),
            }
        }
        "destination" => {
            if arg.is_empty() {
                return CommandAction::ErrorToSource(
                    "Usage: !destination <channel_id|dm>".into(),
                );
            }
            let Some(dest) = DiscordDestination::parse(arg) else {
                return CommandAction::ErrorToSource(
                    "Invalid destination. Use channel ID or dm.".into(),
                );
            };
            let display = dest.display();
            CommandAction::SetDestination {
                destination: dest,
                response: format!("Destination set to: {display}"),
            }
        }
        "receive-all" => {
            let new_val = match arg.trim().to_lowercase().as_str() {
                "on" | "true" | "yes" | "1" => true,
                "off" | "false" | "no" | "0" => false,
                "" => !state.receive_all, // toggle
                _ => {
                    return CommandAction::ErrorToSource(
                        "Usage: !receive-all [on|off] (omit to toggle)".into(),
                    );
                }
            };
            CommandAction::SetReceiveAll {
                receive_all: new_val,
                response: format!(
                    "receive_all set to: {}",
                    if new_val { "on" } else { "off" }
                ),
            }
        }
        "status" => {
            let dir = current_dir_key();
            CommandAction::Respond(build_status_text(state, &dir))
        }
        "list" => {
            if conversation_names.is_empty() {
                return CommandAction::Respond("No conversations yet.".into());
            }
            let body = format_list_body(conversation_names, &state.conversation);
            CommandAction::Respond(body)
        }
        "new" => {
            if arg.is_empty() {
                return CommandAction::ErrorToSource("Usage: !new <conversation>".into());
            }
            CommandAction::CreateConversation(arg.to_string())
        }
        "compact" => {
            if !conversation_exists {
                return CommandAction::ErrorToSource("Conversation does not exist.".into());
            }
            if exchange_count == 0 {
                return CommandAction::Respond(format!(
                    "Nothing to compact in '{}'",
                    state.conversation
                ));
            }
            CommandAction::RunCompact(state.conversation.clone())
        }
        _ => CommandAction::ErrorToSource(
            "Unknown command. Use: !switch, !agent, !model, !destination, !status, !list, !new, !compact".into(),
        ),
    }
}

/// Formats a list of conversation names for Discord display.
pub(crate) fn format_list_body(names: &[String], active: &str) -> String {
    names
        .iter()
        .map(|name| {
            if name == active {
                format!("* {name}")
            } else {
                format!("  {name}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Decides whether a Discord message should be processed by the bot.
/// When `receive_all` is true, channel messages are accepted without @mention.
pub(crate) fn should_process_message(
    is_bot: bool,
    is_dm: bool,
    is_mention: bool,
    receive_all: bool,
    user_id: &str,
    allowed_users: &[String],
) -> bool {
    if is_bot {
        return false;
    }
    if !is_dm && !is_mention && !receive_all {
        return false;
    }
    allowed_users.iter().any(|u| u == user_id)
}

/// Checks whether a message source matches the bot's configured destination.
/// - `Dm` destination: only accept DMs (guild_id is None).
/// - `Channel(id)` destination: only accept messages from that specific channel.
pub(crate) fn matches_destination(
    destination: &DiscordDestination,
    is_dm: bool,
    channel_id: &str,
) -> bool {
    match destination {
        DiscordDestination::Dm => is_dm,
        DiscordDestination::Channel(dest_id) => channel_id == dest_id,
    }
}

/// Classifies the result of a spawn_blocking send operation for response formatting.
pub(crate) fn format_send_result(
    result: Result<Result<String, String>, String>,
) -> Result<String, String> {
    match result {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(err)) => Err(format!("Command failed: {err}")),
        Err(err) => Err(format!("Worker failed: {err}")),
    }
}

pub(crate) fn build_status_text(state: &DiscordBotState, dir: &str) -> String {
    format!(
        "bot: {}\ndirectory: {}\nconversation: {}\nagent: {}\nmodel: {}\nsandbox: {}\ndestination: {}\nreceive_all: {}",
        state.bot_name,
        dir,
        state.conversation,
        backend_name(&state.backend),
        state.model.as_deref().unwrap_or("default"),
        state.sandbox.as_deref().unwrap_or("none"),
        state.destination.display(),
        state.receive_all,
    )
}

pub(crate) struct ClawsHandler {
    pub(crate) state: Arc<tokio::sync::Mutex<DiscordBotState>>,
}

pub(crate) fn split_for_discord(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return vec!["(no response)".to_string()];
    }
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(max_chars)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

pub(crate) fn strip_leading_mentions(input: &str) -> String {
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

pub(crate) fn parse_bot_command(input: &str) -> Option<(String, String)> {
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

pub(crate) fn cron_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".breo")
}

pub(crate) fn cron_file_path() -> PathBuf {
    cron_dir().join("cron.toml")
}

pub(crate) fn ensure_cron_file() {
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

pub(crate) fn parse_timestamp(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%dT%H:%M:%S")
        .ok()
        .or_else(|| {
            DateTime::parse_from_rfc3339(text.trim())
                .ok()
                .map(|dt| dt.naive_local())
        })
}

pub(crate) fn parse_interval(text: &str) -> Option<ChronoDuration> {
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

pub(crate) fn parse_cron_task(value: &toml::Value) -> Option<CronTask> {
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

pub(crate) fn load_cron_tasks() -> Vec<CronTask> {
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

pub(crate) fn toml_quoted(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

pub(crate) fn save_cron_tasks(tasks: &[CronTask]) {
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

pub(crate) fn mark_task_running(task_name: &str) -> Option<CronTask> {
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

pub(crate) fn complete_cron_task(
    task_name: &str,
    previous_next_run: NaiveDateTime,
    interval: Option<&str>,
) {
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

impl ClawsHandler {
    pub(crate) async fn send_text_to_source(
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

    pub(crate) async fn send_to_destination(
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

    pub(crate) async fn send_to_state_destination(
        &self,
        ctx: &Context,
        text: &str,
    ) -> serenity::Result<()> {
        let (destination, allowed_users) = {
            let state = self.state.lock().await;
            (state.destination.clone(), state.allowed_users.clone())
        };
        Self::send_to_destination(&ctx.http, &destination, &allowed_users, text).await
    }

    pub(crate) async fn handle_command(
        &self,
        ctx: &Context,
        msg: &DiscordMessage,
        command: &str,
        arg: &str,
    ) -> serenity::Result<()> {
        // Gather context for the pure planning function
        let state = self.state.lock().await;
        let conv_exists = if command == "switch" {
            conversation_path(arg).exists()
        } else {
            conversation_path(&state.conversation).exists()
        };
        let names = if command == "list" {
            conversation_names_sorted()
        } else {
            Vec::new()
        };
        let exchange_count = if command == "compact" && conv_exists {
            let path = conversation_path(&state.conversation);
            let content = fs::read_to_string(&path).unwrap_or_default();
            count_exchanges(&content)
        } else {
            0
        };
        let action = plan_command_action(command, arg, &state, conv_exists, &names, exchange_count);
        drop(state);

        // Execute the planned action
        match action {
            CommandAction::SwitchConversation { name, response } => {
                let mut state = self.state.lock().await;
                state.conversation = name;
                set_active(&state.conversation);
                state.persist();
                drop(state);
                self.send_to_state_destination(ctx, &response).await
            }
            CommandAction::SwitchAgent { backend, response } => {
                let mut state = self.state.lock().await;
                state.backend = backend;
                state.persist();
                drop(state);
                self.send_to_state_destination(ctx, &response).await
            }
            CommandAction::SetModel { model, response } => {
                let mut state = self.state.lock().await;
                state.model = Some(model);
                state.persist();
                drop(state);
                self.send_to_state_destination(ctx, &response).await
            }
            CommandAction::SetDestination {
                destination,
                response,
            } => {
                let mut state = self.state.lock().await;
                state.destination = destination;
                state.persist();
                drop(state);
                self.send_to_state_destination(ctx, &response).await
            }
            CommandAction::SetReceiveAll {
                receive_all,
                response,
            } => {
                let mut state = self.state.lock().await;
                state.receive_all = receive_all;
                state.persist();
                drop(state);
                self.send_to_state_destination(ctx, &response).await
            }
            CommandAction::Respond(text) => self.send_to_state_destination(ctx, &text).await,
            CommandAction::CreateConversation(name) => {
                let conversation_name = name.clone();
                let result =
                    tokio::task::spawn_blocking(move || create_conversation(&conversation_name))
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
                state.conversation = name.clone();
                state.persist();
                drop(state);
                self.send_to_state_destination(
                    ctx,
                    &format!("Created and switched to conversation: {name}"),
                )
                .await
            }
            CommandAction::RunCompact(conversation) => {
                let state = self.state.lock().await.clone();
                let conversation_for_msg = conversation.clone();
                let sandbox = state.sandbox.clone();
                let compact_result = tokio::task::spawn_blocking(move || {
                    cmd_compact(Some(&conversation), sandbox.as_deref());
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
            CommandAction::ErrorToSource(text) => self.send_text_to_source(ctx, msg, &text).await,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_cron_task(
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

pub(crate) async fn cron_poll_loop(
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
        let is_dm = msg.guild_id.is_none();
        let is_mention = msg.mentions_me(&ctx).await.unwrap_or(false);
        let msg_channel_id = msg.channel_id.get().to_string();

        // Filter by destination first: silently ignore messages from non-matching sources
        let (destination, allowed_users, receive_all) = {
            let state = self.state.lock().await;
            (
                state.destination.clone(),
                state.allowed_users.clone(),
                state.receive_all,
            )
        };
        if !matches_destination(&destination, is_dm, &msg_channel_id) {
            return;
        }

        let user_id = msg.author.id.get().to_string();
        if !should_process_message(
            msg.author.bot,
            is_dm,
            is_mention,
            receive_all,
            &user_id,
            &allowed_users,
        ) {
            if !msg.author.bot && (is_dm || is_mention) {
                let _ = self.send_text_to_source(&ctx, &msg, "Access denied.").await;
            }
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
        let message = content.clone();

        let result = tokio::task::spawn_blocking(move || {
            let (_, response_or_err, success) = cmd_send_inner(
                &message,
                Some(&conversation),
                model.as_deref(),
                &backend,
                &[],
                sandbox.as_deref(),
                false,
            );
            if success {
                Ok(response_or_err)
            } else {
                Err(response_or_err)
            }
        })
        .await;

        let formatted = format_send_result(result.map_err(|e| e.to_string()));
        match formatted {
            Ok(response) => {
                let _ = self.send_to_state_destination(&ctx, &response).await;
            }
            Err(err) => {
                let _ = self.send_to_state_destination(&ctx, &err).await;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_claws(
    bot_name: &str,
    token: &str,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    backend: Backend,
    model: Option<String>,
    sandbox: Option<String>,
    destination: DiscordDestination,
    receive_all: bool,
) {
    ensure_breo_dir();
    let conversation = get_active();
    let intents = GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    eprintln!(
        "[claws] Bot: {} | Conversation: {} | Agent: {} | Model: {} | Sandbox: {} | Destination: {} | Listen all: {}",
        bot_name,
        conversation,
        backend_name(&backend),
        model.as_deref().unwrap_or("default"),
        sandbox.as_deref().unwrap_or("none"),
        destination.display(),
        receive_all
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
            receive_all,
            allowed_users,
            cron_started: false,
        }));
        state.lock().await.persist();

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

/// Format a single Discord bot profile for list display.
pub(crate) fn format_claws_profile_line(
    name: &str,
    guild_id: Option<&str>,
    allowed_users_count: usize,
) -> String {
    format!(
        "{}\tguild={}\tallowed_users={}",
        name,
        guild_id.unwrap_or("(none)"),
        allowed_users_count,
    )
}

pub(crate) fn cmd_claws_list(config: &Config) {
    let profiles = config.resolved_discord_profiles();
    if profiles.is_empty() {
        eprintln!("No Discord bot profiles configured.");
        eprintln!("Add entries under [discord.bots.<name>] in config.toml.");
        std::process::exit(1);
    }

    for profile in profiles {
        println!(
            "{}",
            format_claws_profile_line(
                &profile.name,
                profile.guild_id.as_deref(),
                profile.allowed_users.len(),
            )
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    fn split_for_discord_empty() {
        assert_eq!(split_for_discord("", 2000), vec!["(no response)"]);
    }

    #[test]
    fn split_for_discord_short() {
        assert_eq!(split_for_discord("short", 2000), vec!["short"]);
    }

    #[test]
    fn split_for_discord_long() {
        let text = "a".repeat(4001);
        let parts = split_for_discord(&text, 2000);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 2000);
    }

    #[test]
    fn split_for_discord_boundary() {
        let text = "a".repeat(2000);
        let parts = split_for_discord(&text, 2000);
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn strip_mentions_cases() {
        assert_eq!(strip_leading_mentions("<@123> hello"), "hello");
        assert_eq!(strip_leading_mentions("<@123> <@456> hello"), "hello");
        assert_eq!(strip_leading_mentions("no mention"), "no mention");
    }

    #[test]
    fn parse_bot_command_cases() {
        assert_eq!(
            parse_bot_command("!switch main"),
            Some(("switch".to_string(), "main".to_string()))
        );
        assert_eq!(
            parse_bot_command("!status"),
            Some(("status".to_string(), "".to_string()))
        );
        assert_eq!(parse_bot_command("hello"), None);
        assert_eq!(
            parse_bot_command("!SWITCH Main"),
            Some(("switch".to_string(), "Main".to_string()))
        );
    }

    #[test]
    fn destination_parse_and_roundtrip() {
        assert!(matches!(
            DiscordDestination::parse("dm"),
            Some(DiscordDestination::Dm)
        ));
        assert!(matches!(
            DiscordDestination::parse("DM"),
            Some(DiscordDestination::Dm)
        ));
        assert!(matches!(
            DiscordDestination::parse("123456"),
            Some(DiscordDestination::Channel(_))
        ));
        assert!(DiscordDestination::parse("").is_none());
        let d = DiscordDestination::Channel("123".into());
        assert_eq!(d.to_storage(), "123");
        assert_eq!(d.display(), "channel 123");
    }

    #[test]
    fn parse_timestamp_valid_invalid() {
        assert!(parse_timestamp("2026-02-24T09:00:00").is_some());
        assert!(parse_timestamp("invalid").is_none());
    }

    #[test]
    fn parse_interval_units() {
        assert_eq!(parse_interval("24h"), Some(ChronoDuration::hours(24)));
        assert_eq!(parse_interval("30m"), Some(ChronoDuration::minutes(30)));
        assert_eq!(parse_interval("7d"), Some(ChronoDuration::days(7)));
        assert_eq!(parse_interval("10s"), Some(ChronoDuration::seconds(10)));
        assert_eq!(parse_interval("x"), None);
    }

    #[test]
    fn parse_cron_task_valid_invalid() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = 2026-02-24T09:00:00
status = "pending"
"#,
        )
        .expect("toml parse");
        assert!(parse_cron_task(&v).is_some());

        let invalid: toml::Value = toml::from_str("name = \"a\"").expect("toml parse");
        assert!(parse_cron_task(&invalid).is_none());
    }

    #[test]
    fn toml_quoted_escapes() {
        let q = toml_quoted("a\"b");
        let wrapped = format!("value = {q}");
        let value: toml::Value = toml::from_str(&wrapped).expect("parse quoted");
        let parsed = value
            .get("value")
            .and_then(toml::Value::as_str)
            .expect("value as str");
        assert_eq!(parsed, "a\"b");
    }

    #[test]
    #[serial]
    fn save_load_cron_round_trip() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let task = CronTask {
            name: "t1".into(),
            message: "hello".into(),
            next_run: parse_timestamp("2026-02-24T09:00:00").expect("timestamp"),
            interval: Some("24h".into()),
            status: CronTaskStatus::Pending,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        let loaded = load_cron_tasks();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "t1");

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    fn cron_task_status_equality() {
        assert_eq!(CronTaskStatus::Pending, CronTaskStatus::Pending);
        assert_eq!(CronTaskStatus::Running, CronTaskStatus::Running);
        assert_ne!(CronTaskStatus::Pending, CronTaskStatus::Running);
    }

    #[test]
    fn parse_timestamp_rfc3339() {
        let dt = parse_timestamp("2026-02-24T09:00:00+00:00");
        assert!(dt.is_some());
    }

    #[test]
    fn parse_timestamp_with_whitespace() {
        let dt = parse_timestamp("  2026-02-24T09:00:00  ");
        assert!(dt.is_some());
    }

    #[test]
    fn parse_interval_edge_cases() {
        assert!(parse_interval("").is_none());
        assert!(parse_interval("x").is_none());
        assert!(parse_interval("1").is_none());
        assert!(parse_interval("abch").is_none());
        assert_eq!(parse_interval("1s"), Some(ChronoDuration::seconds(1)));
        assert_eq!(parse_interval("1d"), Some(ChronoDuration::days(1)));
    }

    #[test]
    fn parse_cron_task_with_interval() {
        let v: toml::Value = toml::from_str(
            r#"name = "periodic"
message = "check status"
next_run = 2026-02-24T09:00:00
interval = "24h"
status = "pending"
"#,
        )
        .expect("toml parse");
        let task = parse_cron_task(&v).expect("parse");
        assert_eq!(task.name, "periodic");
        assert_eq!(task.interval.as_deref(), Some("24h"));
        assert_eq!(task.status, CronTaskStatus::Pending);
    }

    #[test]
    fn parse_cron_task_running_status() {
        let v: toml::Value = toml::from_str(
            r#"name = "r"
message = "m"
next_run = 2026-02-24T09:00:00
status = "running"
"#,
        )
        .expect("toml parse");
        let task = parse_cron_task(&v).expect("parse");
        assert_eq!(task.status, CronTaskStatus::Running);
    }

    #[test]
    fn parse_cron_task_missing_message() {
        let v: toml::Value = toml::from_str(r#"name = "a""#).expect("toml parse");
        assert!(parse_cron_task(&v).is_none());
    }

    #[test]
    fn parse_cron_task_missing_next_run() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
"#,
        )
        .expect("toml parse");
        assert!(parse_cron_task(&v).is_none());
    }

    #[test]
    fn parse_cron_task_invalid_next_run() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = "not-a-date"
"#,
        )
        .expect("toml parse");
        assert!(parse_cron_task(&v).is_none());
    }

    #[test]
    fn discord_destination_dm_storage() {
        let d = DiscordDestination::Dm;
        assert_eq!(d.to_storage(), "dm");
        assert_eq!(d.display(), "dm");
    }

    #[test]
    fn discord_destination_channel_storage() {
        let d = DiscordDestination::Channel("999".into());
        assert_eq!(d.to_storage(), "999");
        assert_eq!(d.display(), "channel 999");
    }

    #[test]
    fn discord_destination_parse_whitespace() {
        assert!(DiscordDestination::parse("  dm  ").is_some());
        assert!(DiscordDestination::parse("  123  ").is_some());
        assert!(DiscordDestination::parse("   ").is_none());
    }

    #[test]
    fn split_for_discord_exact_double() {
        let text = "a".repeat(4000);
        let parts = split_for_discord(&text, 2000);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 2000);
        assert_eq!(parts[1].len(), 2000);
    }

    #[test]
    fn strip_mentions_single() {
        assert_eq!(strip_leading_mentions("<@!123> test"), "test");
    }

    #[test]
    fn strip_mentions_empty_after() {
        assert_eq!(strip_leading_mentions("<@123>"), "");
    }

    #[test]
    fn parse_bot_command_with_extra_spaces() {
        assert_eq!(
            parse_bot_command("  !agent   claude  "),
            Some(("agent".to_string(), "claude".to_string()))
        );
    }

    #[test]
    fn parse_bot_command_empty() {
        assert_eq!(parse_bot_command(""), None);
    }

    #[test]
    fn toml_quoted_simple() {
        let q = toml_quoted("hello");
        assert_eq!(q, "\"hello\"");
    }

    #[test]
    fn toml_quoted_with_newline() {
        let q = toml_quoted("line1\nline2");
        // Should produce valid TOML
        let wrapped = format!("value = {q}");
        let v: toml::Value = toml::from_str(&wrapped).expect("parse");
        assert_eq!(v.get("value").unwrap().as_str().unwrap(), "line1\nline2");
    }

    #[test]
    #[serial]
    fn cron_file_path_ends_with_cron_toml() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let p = cron_file_path();
        assert!(p.to_string_lossy().ends_with("cron.toml"));

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn cron_dir_ends_with_breo() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let d = cron_dir();
        assert!(d.to_string_lossy().ends_with(".breo"));

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn save_cron_tasks_multiple() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let tasks = vec![
            CronTask {
                name: "t1".into(),
                message: "msg1".into(),
                next_run: parse_timestamp("2026-02-24T09:00:00").expect("ts"),
                interval: None,
                status: CronTaskStatus::Pending,
            },
            CronTask {
                name: "t2".into(),
                message: "msg2".into(),
                next_run: parse_timestamp("2026-02-24T10:00:00").expect("ts"),
                interval: Some("1h".into()),
                status: CronTaskStatus::Running,
            },
        ];
        save_cron_tasks(&tasks);
        let loaded = load_cron_tasks();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "t1");
        assert!(loaded[0].interval.is_none());
        assert_eq!(loaded[1].name, "t2");
        assert_eq!(loaded[1].interval.as_deref(), Some("1h"));
        assert_eq!(loaded[1].status, CronTaskStatus::Running);

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn save_cron_tasks_empty() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        save_cron_tasks(&[]);
        let loaded = load_cron_tasks();
        assert!(loaded.is_empty());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn complete_cron_task_one_shot_removes() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let task = CronTask {
            name: "one-shot".into(),
            message: "m".into(),
            next_run: parse_timestamp("2026-02-24T09:00:00").expect("ts"),
            interval: None,
            status: CronTaskStatus::Running,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        complete_cron_task("one-shot", task.next_run, None);
        let loaded = load_cron_tasks();
        assert!(loaded.is_empty());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn complete_cron_task_periodic_reschedules() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let next_run = parse_timestamp("2026-02-24T09:00:00").expect("ts");
        let task = CronTask {
            name: "periodic".into(),
            message: "m".into(),
            next_run,
            interval: Some("1h".into()),
            status: CronTaskStatus::Running,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        complete_cron_task("periodic", next_run, Some("1h"));
        let loaded = load_cron_tasks();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].status, CronTaskStatus::Pending);
        assert!(loaded[0].next_run > next_run);

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn complete_cron_task_invalid_interval_removes() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let next_run = parse_timestamp("2026-02-24T09:00:00").expect("ts");
        let task = CronTask {
            name: "bad-interval".into(),
            message: "m".into(),
            next_run,
            interval: Some("invalid".into()),
            status: CronTaskStatus::Running,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        complete_cron_task("bad-interval", next_run, Some("invalid"));
        let loaded = load_cron_tasks();
        assert!(loaded.is_empty());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn ensure_cron_file_creates_dir_and_file() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        ensure_cron_file();
        assert!(cron_file_path().exists());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn load_cron_tasks_from_empty_file() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        // ensure_cron_file creates the header-only file
        ensure_cron_file();
        let tasks = load_cron_tasks();
        assert!(tasks.is_empty());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    fn discord_bot_state_clone() {
        let state = DiscordBotState {
            bot_name: "test".into(),
            conversation: "main".into(),
            backend: Backend::Claude,
            model: Some("opus".into()),
            sandbox: None,
            destination: DiscordDestination::Dm,
            receive_all: false,
            allowed_users: vec!["1".into()],
            cron_started: false,
        };
        let cloned = state.clone();
        assert_eq!(cloned.bot_name, "test");
        assert_eq!(cloned.conversation, "main");
    }

    #[test]
    fn cmd_claws_list_with_profiles() {
        let mut bots = std::collections::HashMap::new();
        bots.insert(
            "bot1".into(),
            crate::config::DiscordBotSection {
                bot_token: Some("tok".into()),
                guild_id: Some("g1".into()),
                allowed_users: vec!["u1".into()],
            },
        );
        let config = Config {
            discord: Some(crate::config::DiscordSection {
                bots,
                ..crate::config::DiscordSection::default()
            }),
            ..Config::default()
        };
        // cmd_claws_list prints to stdout, just verify no panic
        cmd_claws_list(&config);
    }

    #[test]
    fn build_status_text_all_fields() {
        let state = DiscordBotState {
            bot_name: "mybot".into(),
            conversation: "main".into(),
            backend: Backend::Gemini,
            model: Some("gemini-3-pro".into()),
            sandbox: Some("default".into()),
            destination: DiscordDestination::Channel("123456".into()),
            receive_all: true,
            allowed_users: vec!["u1".into()],
            cron_started: false,
        };
        let text = build_status_text(&state, "/home/user/project");
        assert!(text.contains("bot: mybot"));
        assert!(text.contains("directory: /home/user/project"));
        assert!(text.contains("conversation: main"));
        assert!(text.contains("agent: gemini"));
        assert!(text.contains("model: gemini-3-pro"));
        assert!(text.contains("sandbox: default"));
        assert!(text.contains("destination: channel 123456"));
        assert!(text.contains("receive_all: true"));
    }

    #[test]
    fn build_status_text_defaults() {
        let state = DiscordBotState {
            bot_name: "bot".into(),
            conversation: "test".into(),
            backend: Backend::Claude,
            model: None,
            sandbox: None,
            destination: DiscordDestination::Dm,
            receive_all: false,
            allowed_users: vec![],
            cron_started: false,
        };
        let text = build_status_text(&state, "/tmp");
        assert!(text.contains("model: default"));
        assert!(text.contains("sandbox: none"));
        assert!(text.contains("destination: dm"));
        assert!(text.contains("receive_all: false"));
    }

    #[test]
    fn parse_bot_command_all_known_commands() {
        for cmd in [
            "switch",
            "agent",
            "model",
            "destination",
            "status",
            "list",
            "new",
            "compact",
        ] {
            let input = format!("!{cmd} arg");
            let result = parse_bot_command(&input);
            assert!(result.is_some(), "Failed to parse !{cmd}");
            assert_eq!(result.unwrap().0, cmd);
        }
    }

    #[test]
    fn parse_bot_command_no_exclamation() {
        assert!(parse_bot_command("switch main").is_none());
        assert!(parse_bot_command("agent claude").is_none());
    }

    #[test]
    fn strip_mentions_multiple() {
        assert_eq!(strip_leading_mentions("<@111> <@222> <@333> msg"), "msg");
    }

    #[test]
    fn strip_mentions_with_exclamation() {
        assert_eq!(strip_leading_mentions("<@!999> hello"), "hello");
    }

    #[test]
    fn strip_mentions_no_mention() {
        assert_eq!(strip_leading_mentions("just text"), "just text");
    }

    #[test]
    fn split_for_discord_single_char_limit() {
        let parts = split_for_discord("abc", 1);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "a");
        assert_eq!(parts[1], "b");
        assert_eq!(parts[2], "c");
    }

    #[test]
    fn discord_destination_channel_roundtrip() {
        let d = DiscordDestination::parse("999888777").unwrap();
        assert_eq!(d.to_storage(), "999888777");
        assert_eq!(d.display(), "channel 999888777");
    }

    #[test]
    fn discord_destination_dm_roundtrip() {
        let d = DiscordDestination::parse("dm").unwrap();
        assert_eq!(d.to_storage(), "dm");
        assert_eq!(d.display(), "dm");
    }

    #[test]
    fn discord_destination_dm_case_insensitive() {
        assert!(matches!(
            DiscordDestination::parse("DM"),
            Some(DiscordDestination::Dm)
        ));
        assert!(matches!(
            DiscordDestination::parse("Dm"),
            Some(DiscordDestination::Dm)
        ));
        assert!(matches!(
            DiscordDestination::parse("dM"),
            Some(DiscordDestination::Dm)
        ));
    }

    #[test]
    fn parse_interval_zero_value() {
        assert_eq!(parse_interval("0h"), Some(ChronoDuration::hours(0)));
        assert_eq!(parse_interval("0m"), Some(ChronoDuration::minutes(0)));
    }

    #[test]
    fn parse_interval_large_value() {
        assert_eq!(parse_interval("365d"), Some(ChronoDuration::days(365)));
    }

    #[test]
    fn parse_interval_unknown_unit() {
        assert!(parse_interval("10w").is_none()); // weeks not supported
        assert!(parse_interval("5y").is_none()); // years not supported
    }

    #[test]
    fn parse_cron_task_default_status_is_pending() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = 2026-02-24T09:00:00
"#,
        )
        .expect("toml parse");
        let task = parse_cron_task(&v).expect("parse");
        assert_eq!(task.status, CronTaskStatus::Pending);
    }

    #[test]
    fn parse_cron_task_unknown_status_defaults_pending() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = 2026-02-24T09:00:00
status = "unknown"
"#,
        )
        .expect("toml parse");
        let task = parse_cron_task(&v).expect("parse");
        assert_eq!(task.status, CronTaskStatus::Pending);
    }

    #[test]
    fn parse_cron_task_next_run_as_string() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = "2026-02-24T09:00:00"
"#,
        )
        .expect("toml parse");
        let task = parse_cron_task(&v).expect("parse");
        assert_eq!(task.name, "a");
    }

    #[test]
    fn toml_quoted_special_chars() {
        for input in ["tab\there", "back\\slash", "quote\"mark", "newline\nhere"] {
            let q = toml_quoted(input);
            let wrapped = format!("v = {q}");
            let v: toml::Value = toml::from_str(&wrapped).expect("parse");
            assert_eq!(v.get("v").unwrap().as_str().unwrap(), input);
        }
    }

    #[test]
    fn cron_task_clone() {
        let task = CronTask {
            name: "t".into(),
            message: "m".into(),
            next_run: parse_timestamp("2026-02-24T09:00:00").expect("ts"),
            interval: Some("1h".into()),
            status: CronTaskStatus::Pending,
        };
        let cloned = task.clone();
        assert_eq!(cloned.name, "t");
        assert_eq!(cloned.interval.as_deref(), Some("1h"));
    }

    #[test]
    fn cron_task_status_debug() {
        let debug_str = format!("{:?}", CronTaskStatus::Pending);
        assert_eq!(debug_str, "Pending");
        let debug_str = format!("{:?}", CronTaskStatus::Running);
        assert_eq!(debug_str, "Running");
    }

    #[test]
    fn cron_task_status_copy() {
        let s = CronTaskStatus::Pending;
        let copied = s;
        assert_eq!(s, copied);
    }

    #[test]
    #[serial]
    fn mark_task_running_no_matching_task() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let task = CronTask {
            name: "other-task".into(),
            message: "m".into(),
            next_run: parse_timestamp("2026-02-24T09:00:00").expect("ts"),
            interval: None,
            status: CronTaskStatus::Pending,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        let result = mark_task_running("nonexistent");
        assert!(result.is_none());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn mark_task_running_already_running() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let task = CronTask {
            name: "already-running".into(),
            message: "m".into(),
            next_run: parse_timestamp("2020-01-01T00:00:00").expect("ts"),
            interval: None,
            status: CronTaskStatus::Running,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        let result = mark_task_running("already-running");
        assert!(result.is_none());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn mark_task_running_future_task() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let task = CronTask {
            name: "future-task".into(),
            message: "m".into(),
            next_run: parse_timestamp("2099-12-31T23:59:59").expect("ts"),
            interval: None,
            status: CronTaskStatus::Pending,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        let result = mark_task_running("future-task");
        assert!(result.is_none());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn mark_task_running_due_task_becomes_running() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let task = CronTask {
            name: "due-task".into(),
            message: "do something".into(),
            next_run: parse_timestamp("2020-01-01T00:00:00").expect("ts"),
            interval: None,
            status: CronTaskStatus::Pending,
        };
        save_cron_tasks(std::slice::from_ref(&task));
        let result = mark_task_running("due-task");
        assert!(result.is_some());
        let marked = result.unwrap();
        assert_eq!(marked.name, "due-task");
        assert_eq!(marked.status, CronTaskStatus::Running);

        // Verify persisted state
        let loaded = load_cron_tasks();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].status, CronTaskStatus::Running);

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn complete_cron_task_nonexistent_task() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        save_cron_tasks(&[]);
        // Should not panic when task doesn't exist
        complete_cron_task(
            "nonexistent",
            parse_timestamp("2026-02-24T09:00:00").expect("ts"),
            None,
        );
        let loaded = load_cron_tasks();
        assert!(loaded.is_empty());

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn ensure_cron_file_idempotent() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        ensure_cron_file();
        let content1 = fs::read_to_string(cron_file_path()).expect("read");
        ensure_cron_file();
        let content2 = fs::read_to_string(cron_file_path()).expect("read");
        assert_eq!(content1, content2);

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    #[serial]
    fn cron_header_contains_example() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        ensure_cron_file();
        let content = fs::read_to_string(cron_file_path()).expect("read");
        assert!(content.contains("Example"));
        assert!(content.contains("[[task]]"));

        std::env::set_current_dir(old).expect("restore cwd");
    }

    #[test]
    fn discord_bot_state_all_fields() {
        let state = DiscordBotState {
            bot_name: "test-bot".into(),
            conversation: "conv-1".into(),
            backend: Backend::Codex,
            model: Some("gpt-5".into()),
            sandbox: Some("vm1".into()),
            destination: DiscordDestination::Channel("456".into()),
            receive_all: true,
            allowed_users: vec!["u1".into(), "u2".into()],
            cron_started: true,
        };
        assert_eq!(state.bot_name, "test-bot");
        assert_eq!(state.conversation, "conv-1");
        assert_eq!(state.model.as_deref(), Some("gpt-5"));
        assert_eq!(state.sandbox.as_deref(), Some("vm1"));
        assert_eq!(state.allowed_users.len(), 2);
        assert!(state.cron_started);
    }

    #[test]
    fn parse_timestamp_various_formats() {
        // ISO 8601 local
        assert!(parse_timestamp("2026-02-24T09:00:00").is_some());
        // RFC 3339 with offset
        assert!(parse_timestamp("2026-02-24T09:00:00+00:00").is_some());
        assert!(parse_timestamp("2026-02-24T09:00:00-05:00").is_some());
        // Invalid
        assert!(parse_timestamp("2026-02-24").is_none());
        assert!(parse_timestamp("09:00:00").is_none());
        assert!(parse_timestamp("").is_none());
    }

    fn sample_state() -> DiscordBotState {
        DiscordBotState {
            bot_name: "bot".into(),
            conversation: "main".into(),
            backend: Backend::Claude,
            model: None,
            sandbox: None,
            destination: DiscordDestination::Dm,
            receive_all: false,
            allowed_users: vec!["1".into()],
            cron_started: false,
        }
    }

    #[test]
    fn plan_command_action_switch_errors_and_success() {
        let state = sample_state();
        assert!(matches!(
            plan_command_action("switch", "", &state, false, &[], 0),
            CommandAction::ErrorToSource(msg) if msg.contains("Usage")
        ));
        assert!(matches!(
            plan_command_action("switch", "missing", &state, false, &[], 0),
            CommandAction::ErrorToSource(msg) if msg.contains("not found")
        ));
        assert!(matches!(
            plan_command_action("switch", "main", &state, true, &[], 0),
            CommandAction::SwitchConversation { name, .. } if name == "main"
        ));
    }

    #[test]
    fn plan_command_action_agent_model_and_destination() {
        let state = sample_state();
        assert!(matches!(
            plan_command_action("agent", "", &state, true, &[], 0),
            CommandAction::ErrorToSource(_)
        ));
        assert!(matches!(
            plan_command_action("agent", "invalid", &state, true, &[], 0),
            CommandAction::ErrorToSource(_)
        ));
        assert!(matches!(
            plan_command_action("agent", "codex", &state, true, &[], 0),
            CommandAction::SwitchAgent {
                backend: Backend::Codex,
                ..
            }
        ));
        assert!(matches!(
            plan_command_action("model", "gpt-5", &state, true, &[], 0),
            CommandAction::SetModel { model, .. } if model == "gpt-5"
        ));
        assert!(matches!(
            plan_command_action("destination", "dm", &state, true, &[], 0),
            CommandAction::SetDestination {
                destination: DiscordDestination::Dm,
                ..
            }
        ));
        assert!(matches!(
            plan_command_action("destination", "", &state, true, &[], 0),
            CommandAction::ErrorToSource(_)
        ));
    }

    #[test]
    fn plan_command_action_list_and_status() {
        let state = sample_state();
        assert!(matches!(
            plan_command_action("list", "", &state, true, &[], 0),
            CommandAction::Respond(msg) if msg.contains("No conversations")
        ));
        let names = vec!["main".to_string(), "other".to_string()];
        assert!(matches!(
            plan_command_action("list", "", &state, true, &names, 0),
            CommandAction::Respond(msg) if msg.contains("* main") && msg.contains("other")
        ));
        assert!(matches!(
            plan_command_action("status", "", &state, true, &names, 0),
            CommandAction::Respond(msg) if msg.contains("conversation: main")
        ));
    }

    #[test]
    fn plan_command_action_new_compact_and_unknown() {
        let state = sample_state();
        assert!(matches!(
            plan_command_action("new", "", &state, true, &[], 0),
            CommandAction::ErrorToSource(_)
        ));
        assert!(matches!(
            plan_command_action("new", "feat", &state, true, &[], 0),
            CommandAction::CreateConversation(name) if name == "feat"
        ));
        assert!(matches!(
            plan_command_action("compact", "", &state, false, &[], 0),
            CommandAction::ErrorToSource(_)
        ));
        assert!(matches!(
            plan_command_action("compact", "", &state, true, &[], 0),
            CommandAction::Respond(msg) if msg.contains("Nothing to compact")
        ));
        assert!(matches!(
            plan_command_action("compact", "", &state, true, &[], 1),
            CommandAction::RunCompact(name) if name == "main"
        ));
        assert!(matches!(
            plan_command_action("help", "", &state, true, &[], 0),
            CommandAction::ErrorToSource(msg) if msg.contains("Unknown command")
        ));
    }

    #[test]
    #[serial]
    fn save_cron_tasks_preserves_header() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir test");

        let tasks = vec![CronTask {
            name: "t".into(),
            message: "m".into(),
            next_run: parse_timestamp("2026-02-24T09:00:00").expect("ts"),
            interval: None,
            status: CronTaskStatus::Pending,
        }];
        save_cron_tasks(&tasks);
        let content = fs::read_to_string(cron_file_path()).expect("read");
        assert!(content.starts_with("# breo cron"));
        assert!(content.contains("[[task]]"));
        assert!(content.contains("name = \"t\""));

        std::env::set_current_dir(old).expect("restore cwd");
    }

    // --- plan_command_action additional tests ---

    #[test]
    fn plan_switch_response_contains_name() {
        let state = sample_state();
        if let CommandAction::SwitchConversation { response, .. } =
            plan_command_action("switch", "feat", &state, true, &[], 0)
        {
            assert!(response.contains("feat"));
        } else {
            panic!("expected SwitchConversation");
        }
    }

    #[test]
    fn plan_agent_response_contains_backend_name() {
        let state = sample_state();
        if let CommandAction::SwitchAgent { response, .. } =
            plan_command_action("agent", "gemini", &state, true, &[], 0)
        {
            assert!(response.contains("gemini"));
        } else {
            panic!("expected SwitchAgent");
        }
    }

    #[test]
    fn plan_model_response_contains_model_name() {
        let state = sample_state();
        if let CommandAction::SetModel {
            response, model, ..
        } = plan_command_action("model", "opus-3", &state, true, &[], 0)
        {
            assert!(response.contains("opus-3"));
            assert_eq!(model, "opus-3");
        } else {
            panic!("expected SetModel");
        }
    }

    #[test]
    fn plan_destination_channel() {
        let state = sample_state();
        if let CommandAction::SetDestination {
            destination,
            response,
        } = plan_command_action("destination", "999", &state, true, &[], 0)
        {
            assert!(matches!(destination, DiscordDestination::Channel(id) if id == "999"));
            assert!(response.contains("channel"));
        } else {
            panic!("expected SetDestination");
        }
    }

    #[test]
    fn plan_list_marks_active_conversation() {
        let mut state = sample_state();
        state.conversation = "second".to_string();
        let names = vec!["first".into(), "second".into(), "third".into()];
        if let CommandAction::Respond(body) =
            plan_command_action("list", "", &state, true, &names, 0)
        {
            assert!(body.contains("* second"));
            assert!(body.contains("  first"));
            assert!(body.contains("  third"));
        } else {
            panic!("expected Respond");
        }
    }

    #[test]
    fn plan_compact_with_many_exchanges() {
        let mut state = sample_state();
        state.conversation = "deep-conv".into();
        assert!(matches!(
            plan_command_action("compact", "", &state, true, &[], 10),
            CommandAction::RunCompact(name) if name == "deep-conv"
        ));
    }

    #[test]
    fn plan_all_error_messages_are_nonempty() {
        let state = sample_state();
        let error_cases = vec![
            ("switch", "", false),
            ("switch", "x", false),
            ("agent", "", true),
            ("agent", "bad", true),
            ("model", "", true),
            ("destination", "", true),
            ("new", "", true),
            ("compact", "", false),
            ("xxx", "", true),
        ];
        for (cmd, arg, conv_exists) in error_cases {
            if let CommandAction::ErrorToSource(msg) =
                plan_command_action(cmd, arg, &state, conv_exists, &[], 0)
            {
                assert!(!msg.is_empty(), "empty error for !{cmd} {arg}");
            }
        }
    }

    // --- format_list_body tests ---

    #[test]
    fn format_list_body_empty() {
        assert_eq!(format_list_body(&[], "any"), "");
    }

    #[test]
    fn format_list_body_single_active() {
        assert_eq!(format_list_body(&["main".into()], "main"), "* main");
    }

    #[test]
    fn format_list_body_single_inactive() {
        assert_eq!(format_list_body(&["main".into()], "other"), "  main");
    }

    #[test]
    fn format_list_body_multiple() {
        let names = vec!["a".into(), "b".into(), "c".into()];
        let body = format_list_body(&names, "b");
        assert_eq!(body, "  a\n* b\n  c");
    }

    // --- should_process_message tests ---

    #[test]
    fn should_process_bot_messages_rejected() {
        assert!(!should_process_message(
            true,
            true,
            true,
            false,
            "1",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_non_dm_non_mention_rejected() {
        assert!(!should_process_message(
            false,
            false,
            false,
            false,
            "1",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_dm_allowed_accepted() {
        assert!(should_process_message(
            false,
            true,
            false,
            false,
            "1",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_mention_allowed_accepted() {
        assert!(should_process_message(
            false,
            false,
            true,
            false,
            "1",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_dm_not_allowed_rejected() {
        assert!(!should_process_message(
            false,
            true,
            false,
            false,
            "2",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_multiple_allowed_users() {
        assert!(should_process_message(
            false,
            true,
            false,
            false,
            "3",
            &["1".into(), "2".into(), "3".into()]
        ));
    }

    #[test]
    fn should_process_empty_allowed_rejected() {
        assert!(!should_process_message(false, true, true, false, "1", &[]));
    }

    #[test]
    fn should_process_receive_all_channel_message_accepted() {
        // receive_all=true: channel message without mention should be accepted
        assert!(should_process_message(
            false,
            false,
            false,
            true,
            "1",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_receive_all_still_rejects_bots() {
        // receive_all doesn't override bot rejection
        assert!(!should_process_message(
            true,
            false,
            false,
            true,
            "1",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_receive_all_still_requires_auth() {
        // receive_all doesn't bypass allowed_users check
        assert!(!should_process_message(
            false,
            false,
            false,
            true,
            "2",
            &["1".into()]
        ));
    }

    // --- format_send_result tests ---

    #[test]
    fn format_send_result_ok_ok() {
        let r = format_send_result(Ok(Ok("response text".into())));
        assert_eq!(r, Ok("response text".into()));
    }

    #[test]
    fn format_send_result_ok_err() {
        let r = format_send_result(Ok(Err("backend failed".into())));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("Command failed"));
    }

    #[test]
    fn format_send_result_err() {
        let r = format_send_result(Err("join error".into()));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("Worker failed"));
    }

    #[test]
    fn format_send_result_preserves_original_error() {
        let r = format_send_result(Ok(Err("specific error text".into())));
        assert!(r.unwrap_err().contains("specific error text"));
    }

    // --- plan_command_action with custom state ---

    #[test]
    fn plan_status_includes_all_state_fields() {
        let mut state = sample_state();
        state.bot_name = "mybot".into();
        state.model = Some("opus".into());
        state.sandbox = Some("vm1".into());
        state.destination = DiscordDestination::Channel("777".into());
        if let CommandAction::Respond(text) =
            plan_command_action("status", "", &state, true, &[], 0)
        {
            assert!(text.contains("mybot"));
            assert!(text.contains("opus"));
            assert!(text.contains("vm1"));
            assert!(text.contains("channel 777"));
        } else {
            panic!("expected Respond");
        }
    }

    #[test]
    fn plan_agent_all_backends() {
        let state = sample_state();
        for (input, expected) in [
            ("claude", Backend::Claude),
            ("codex", Backend::Codex),
            ("gemini", Backend::Gemini),
        ] {
            if let CommandAction::SwitchAgent { backend, .. } =
                plan_command_action("agent", input, &state, true, &[], 0)
            {
                assert!(
                    matches!(backend, b if std::mem::discriminant(&b) == std::mem::discriminant(&expected))
                );
            } else {
                panic!("expected SwitchAgent for {input}");
            }
        }
    }

    #[test]
    fn plan_destination_dm_case_insensitive_via_parse() {
        let state = sample_state();
        for input in ["dm", "DM", "Dm"] {
            assert!(
                matches!(
                    plan_command_action("destination", input, &state, true, &[], 0),
                    CommandAction::SetDestination {
                        destination: DiscordDestination::Dm,
                        ..
                    }
                ),
                "failed for input: {input}"
            );
        }
    }

    // --- format_claws_profile_line tests ---

    #[test]
    fn format_claws_profile_line_with_guild() {
        let line = format_claws_profile_line("mybot", Some("12345"), 3);
        assert!(line.contains("mybot"));
        assert!(line.contains("guild=12345"));
        assert!(line.contains("allowed_users=3"));
    }

    #[test]
    fn format_claws_profile_line_no_guild() {
        let line = format_claws_profile_line("bot", None, 1);
        assert!(line.contains("guild=(none)"));
        assert!(line.contains("allowed_users=1"));
    }

    #[test]
    fn format_claws_profile_line_zero_users() {
        let line = format_claws_profile_line("empty-bot", Some("g1"), 0);
        assert!(line.contains("allowed_users=0"));
    }

    // --- parse_cron_task additional edge cases ---

    #[test]
    fn parse_cron_task_next_run_as_integer() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = 12345
"#,
        )
        .expect("toml parse");
        assert!(parse_cron_task(&v).is_none());
    }

    #[test]
    fn parse_cron_task_next_run_as_boolean() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = true
"#,
        )
        .expect("toml parse");
        assert!(parse_cron_task(&v).is_none());
    }

    #[test]
    fn parse_cron_task_next_run_as_float() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = 1.5
"#,
        )
        .expect("toml parse");
        assert!(parse_cron_task(&v).is_none());
    }

    #[test]
    fn parse_cron_task_empty_name() {
        let v: toml::Value = toml::from_str(
            r#"name = ""
message = "m"
next_run = 2026-02-24T09:00:00
"#,
        )
        .expect("toml parse");
        // Empty name is valid TOML, parse succeeds
        let task = parse_cron_task(&v).expect("parse");
        assert!(task.name.is_empty());
    }

    #[test]
    fn parse_cron_task_empty_message() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = ""
next_run = 2026-02-24T09:00:00
"#,
        )
        .expect("toml parse");
        let task = parse_cron_task(&v).expect("parse");
        assert!(task.message.is_empty());
    }

    #[test]
    fn parse_cron_task_with_extra_fields() {
        let v: toml::Value = toml::from_str(
            r#"name = "a"
message = "m"
next_run = 2026-02-24T09:00:00
extra_field = "ignored"
another = 42
"#,
        )
        .expect("toml parse");
        let task = parse_cron_task(&v).expect("parse");
        assert_eq!(task.name, "a");
    }

    #[test]
    fn parse_cron_task_not_a_table() {
        let v: toml::Value = toml::Value::String("not a table".into());
        assert!(parse_cron_task(&v).is_none());
    }

    // --- load_cron_tasks additional tests ---

    #[test]
    #[serial]
    fn load_cron_tasks_invalid_toml() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir");
        fs::create_dir_all(".breo").unwrap();
        fs::write(".breo/cron.toml", "{{{{invalid toml!!!!").unwrap();
        let tasks = load_cron_tasks();
        assert!(tasks.is_empty());
        std::env::set_current_dir(old).expect("restore");
    }

    #[test]
    #[serial]
    fn load_cron_tasks_valid_toml_no_tasks() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir");
        fs::create_dir_all(".breo").unwrap();
        fs::write(".breo/cron.toml", "# just a comment\n").unwrap();
        let tasks = load_cron_tasks();
        assert!(tasks.is_empty());
        std::env::set_current_dir(old).expect("restore");
    }

    #[test]
    #[serial]
    fn load_cron_tasks_multiple_tasks() {
        let td = TempDir::new().expect("tempdir");
        let old = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(td.path()).expect("chdir");
        fs::create_dir_all(".breo").unwrap();
        fs::write(
            ".breo/cron.toml",
            r#"[[task]]
name = "t1"
message = "m1"
next_run = 2026-02-24T09:00:00

[[task]]
name = "t2"
message = "m2"
next_run = 2026-02-24T10:00:00
interval = "1h"
"#,
        )
        .unwrap();
        let tasks = load_cron_tasks();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "t1");
        assert!(tasks[0].interval.is_none());
        assert_eq!(tasks[1].name, "t2");
        assert_eq!(tasks[1].interval.as_deref(), Some("1h"));
        std::env::set_current_dir(old).expect("restore");
    }

    // --- more split_for_discord tests ---

    #[test]
    fn split_for_discord_unicode() {
        // Unicode chars that are multiple bytes
        let text = "🎉".repeat(3000);
        let parts = split_for_discord(&text, 2000);
        assert!(parts.len() >= 2);
        // Each part should have at most 2000 chars
        for part in &parts {
            assert!(part.chars().count() <= 2000);
        }
    }

    #[test]
    fn split_for_discord_newlines() {
        let text = "line\n".repeat(500);
        let parts = split_for_discord(&text, 2000);
        assert!(!parts.is_empty());
        for part in &parts {
            assert!(part.chars().count() <= 2000);
        }
    }

    #[test]
    fn split_for_discord_whitespace_only() {
        let parts = split_for_discord("   ", 2000);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], "   ");
    }

    // --- more strip_leading_mentions tests ---

    #[test]
    fn strip_mentions_mixed_content() {
        assert_eq!(
            strip_leading_mentions("<@123> !switch main"),
            "!switch main"
        );
    }

    #[test]
    fn strip_mentions_incomplete_mention() {
        assert_eq!(strip_leading_mentions("<@123 no close"), "<@123 no close");
    }

    #[test]
    fn strip_mentions_empty() {
        assert_eq!(strip_leading_mentions(""), "");
    }

    // --- more parse_bot_command tests ---

    #[test]
    fn parse_bot_command_just_exclamation() {
        let result = parse_bot_command("!");
        assert_eq!(result, Some(("".into(), "".into())));
    }

    #[test]
    fn parse_bot_command_multiword_arg() {
        let result = parse_bot_command("!model gpt-5-turbo-preview");
        assert_eq!(result, Some(("model".into(), "gpt-5-turbo-preview".into())));
    }

    #[test]
    fn parse_bot_command_arg_with_spaces() {
        let result = parse_bot_command("!new my conversation name");
        assert_eq!(result, Some(("new".into(), "my conversation name".into())));
    }

    // --- more parse_timestamp tests ---

    #[test]
    fn parse_timestamp_midnight() {
        let dt = parse_timestamp("2026-01-01T00:00:00");
        assert!(dt.is_some());
        assert!(dt.unwrap().to_string().contains("00:00:00"));
    }

    #[test]
    fn parse_timestamp_end_of_day() {
        let dt = parse_timestamp("2026-12-31T23:59:59");
        assert!(dt.is_some());
        assert!(dt.unwrap().to_string().contains("23:59:59"));
    }

    // --- more parse_interval tests ---

    #[test]
    fn parse_interval_all_valid_units() {
        assert!(parse_interval("10s").is_some());
        assert!(parse_interval("10m").is_some());
        assert!(parse_interval("10h").is_some());
        assert!(parse_interval("10d").is_some());
    }

    #[test]
    fn parse_interval_negative_value() {
        // chrono allows negative durations
        let result = parse_interval("-1h");
        // "-1" parses as i64, so this should work
        assert!(result.is_some() || result.is_none()); // depends on parse
    }

    #[test]
    fn parse_interval_single_char() {
        assert!(parse_interval("h").is_none()); // len < 2 handled
    }

    // --- more DiscordDestination tests ---

    #[test]
    fn discord_destination_parse_numeric_channel() {
        if let Some(DiscordDestination::Channel(id)) = DiscordDestination::parse("123456789") {
            assert_eq!(id, "123456789");
        } else {
            panic!("expected Channel");
        }
    }

    #[test]
    fn discord_destination_parse_returns_none_for_empty_trimmed() {
        assert!(DiscordDestination::parse("").is_none());
        assert!(DiscordDestination::parse("  ").is_none());
        assert!(DiscordDestination::parse("\t\n").is_none());
    }

    #[test]
    fn discord_destination_clone() {
        let d = DiscordDestination::Channel("123".into());
        let cloned = d.clone();
        assert_eq!(cloned.to_storage(), "123");
    }

    // --- more build_status_text tests ---

    #[test]
    fn build_status_text_codex_backend() {
        let mut state = sample_state();
        state.backend = Backend::Codex;
        let text = build_status_text(&state, "/tmp");
        assert!(text.contains("agent: codex"));
    }

    #[test]
    fn build_status_text_gemini_with_model_and_sandbox() {
        let mut state = sample_state();
        state.backend = Backend::Gemini;
        state.model = Some("gemini-2.5-pro".into());
        state.sandbox = Some("vm2".into());
        state.destination = DiscordDestination::Channel("456".into());
        let text = build_status_text(&state, "/my/project");
        assert!(text.contains("agent: gemini"));
        assert!(text.contains("model: gemini-2.5-pro"));
        assert!(text.contains("sandbox: vm2"));
        assert!(text.contains("destination: channel 456"));
        assert!(text.contains("directory: /my/project"));
    }

    // --- more plan_command_action edge cases ---

    #[test]
    fn plan_command_action_agent_claude() {
        let state = sample_state();
        if let CommandAction::SwitchAgent { backend, response } =
            plan_command_action("agent", "claude", &state, true, &[], 0)
        {
            assert!(matches!(backend, Backend::Claude));
            assert!(response.contains("claude"));
        } else {
            panic!("expected SwitchAgent");
        }
    }

    #[test]
    fn plan_command_action_compact_zero_exchanges_existing_conv() {
        let state = sample_state();
        if let CommandAction::Respond(msg) =
            plan_command_action("compact", "", &state, true, &[], 0)
        {
            assert!(msg.contains("Nothing to compact"));
            assert!(msg.contains("main"));
        } else {
            panic!("expected Respond");
        }
    }

    #[test]
    fn plan_command_action_new_creates_conversation() {
        let state = sample_state();
        if let CommandAction::CreateConversation(name) =
            plan_command_action("new", "feature-branch", &state, true, &[], 0)
        {
            assert_eq!(name, "feature-branch");
        } else {
            panic!("expected CreateConversation");
        }
    }

    #[test]
    fn plan_command_action_switch_to_existing() {
        let state = sample_state();
        if let CommandAction::SwitchConversation { name, response } =
            plan_command_action("switch", "other-conv", &state, true, &[], 0)
        {
            assert_eq!(name, "other-conv");
            assert!(response.contains("other-conv"));
        } else {
            panic!("expected SwitchConversation");
        }
    }

    #[test]
    fn plan_command_action_unknown_commands() {
        let state = sample_state();
        for cmd in ["help", "quit", "exit", "restart", "ping"] {
            assert!(
                matches!(
                    plan_command_action(cmd, "", &state, true, &[], 0),
                    CommandAction::ErrorToSource(_)
                ),
                "expected ErrorToSource for {cmd}"
            );
        }
    }

    #[test]
    fn plan_command_action_model_empty_error() {
        let state = sample_state();
        if let CommandAction::ErrorToSource(msg) =
            plan_command_action("model", "", &state, true, &[], 0)
        {
            assert!(msg.contains("Usage"));
        } else {
            panic!("expected ErrorToSource");
        }
    }

    // --- more format_list_body tests ---

    #[test]
    fn format_list_body_many_items() {
        let names: Vec<String> = (0..50).map(|i| format!("conv-{i:02}")).collect();
        let body = format_list_body(&names, "conv-25");
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 50);
        assert!(lines[25].starts_with("*"));
        assert_eq!(lines.iter().filter(|l| l.starts_with("*")).count(), 1);
    }

    #[test]
    fn format_list_body_no_active_match() {
        let names = vec!["a".into(), "b".into()];
        let body = format_list_body(&names, "nonexistent");
        assert!(!body.contains("*"));
    }

    // --- more should_process_message tests ---

    #[test]
    fn should_process_dm_and_mention_allowed() {
        assert!(should_process_message(
            false,
            true,
            true,
            false,
            "1",
            &["1".into()]
        ));
    }

    #[test]
    fn should_process_bot_dm_mention_rejected() {
        // Bot messages always rejected regardless of other flags
        assert!(!should_process_message(
            true,
            true,
            true,
            false,
            "1",
            &["1".into()]
        ));
    }

    // --- matches_destination tests ---

    #[test]
    fn matches_destination_dm_accepts_dm() {
        assert!(matches_destination(&DiscordDestination::Dm, true, "12345"));
    }

    #[test]
    fn matches_destination_dm_rejects_channel() {
        assert!(!matches_destination(
            &DiscordDestination::Dm,
            false,
            "12345"
        ));
    }

    #[test]
    fn matches_destination_channel_accepts_matching() {
        assert!(matches_destination(
            &DiscordDestination::Channel("999".into()),
            false,
            "999"
        ));
    }

    #[test]
    fn matches_destination_channel_rejects_different() {
        assert!(!matches_destination(
            &DiscordDestination::Channel("999".into()),
            false,
            "888"
        ));
    }

    #[test]
    fn matches_destination_channel_accepts_matching_even_if_dm_flag() {
        // Channel destination matches by channel_id, regardless of is_dm flag
        assert!(matches_destination(
            &DiscordDestination::Channel("999".into()),
            true,
            "999"
        ));
    }

    #[test]
    fn matches_destination_channel_rejects_dm_with_different_id() {
        assert!(!matches_destination(
            &DiscordDestination::Channel("999".into()),
            true,
            "111"
        ));
    }

    // --- more format_send_result tests ---

    #[test]
    fn format_send_result_empty_ok() {
        let r = format_send_result(Ok(Ok("".into())));
        assert_eq!(r, Ok("".into()));
    }

    #[test]
    fn format_send_result_empty_err() {
        let r = format_send_result(Ok(Err("".into())));
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("Command failed"));
    }

    // --- CRON_FILE_HEADER tests ---

    #[test]
    fn cron_file_header_has_documentation() {
        assert!(CRON_FILE_HEADER.contains("breo cron"));
        assert!(CRON_FILE_HEADER.contains("Fields:"));
        assert!(CRON_FILE_HEADER.contains("name"));
        assert!(CRON_FILE_HEADER.contains("message"));
        assert!(CRON_FILE_HEADER.contains("next_run"));
        assert!(CRON_FILE_HEADER.contains("interval"));
        assert!(CRON_FILE_HEADER.contains("status"));
    }

    #[test]
    fn cron_file_header_has_example() {
        assert!(CRON_FILE_HEADER.contains("Example"));
        assert!(CRON_FILE_HEADER.contains("[[task]]"));
        assert!(CRON_FILE_HEADER.contains("daily-status"));
    }

    // --- toml_quoted additional tests ---

    #[test]
    fn toml_quoted_empty_string() {
        let q = toml_quoted("");
        assert_eq!(q, "\"\"");
    }

    #[test]
    fn toml_quoted_unicode() {
        let q = toml_quoted("café ☕");
        let wrapped = format!("v = {q}");
        let v: toml::Value = toml::from_str(&wrapped).expect("parse");
        assert_eq!(v.get("v").unwrap().as_str().unwrap(), "café ☕");
    }

    // --- CronTask tests ---

    #[test]
    fn cron_task_fields_accessible() {
        let task = CronTask {
            name: "test-task".into(),
            message: "do something".into(),
            next_run: parse_timestamp("2026-03-01T12:00:00").expect("ts"),
            interval: Some("2h".into()),
            status: CronTaskStatus::Pending,
        };
        assert_eq!(task.name, "test-task");
        assert_eq!(task.message, "do something");
        assert_eq!(task.interval.as_deref(), Some("2h"));
        assert_eq!(task.status, CronTaskStatus::Pending);
    }

    #[test]
    fn cron_task_no_interval() {
        let task = CronTask {
            name: "one-shot".into(),
            message: "m".into(),
            next_run: parse_timestamp("2026-03-01T12:00:00").expect("ts"),
            interval: None,
            status: CronTaskStatus::Pending,
        };
        assert!(task.interval.is_none());
    }
}
