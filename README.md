# breo

A CLI tool for working with code agents.

breo orchestrates LLM-powered code agents, storing all interactions as versioned
markdown files. It supports multiple backends (Claude, Codex, Gemini), runs
agents in sandboxed Lima VMs, scopes conversations per project directory, and
provides an automated implement/validate loop for agentic coding workflows.

## Installation

### Quick install (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/antonmry/breo/main/scripts/install.sh | bash
```

This downloads the latest binary for your platform, installs it to
`~/.local/bin`, and sets up shell completions.

| Variable           | Default        | Description                |
| ------------------ | -------------- | -------------------------- |
| `BREO_INSTALL_DIR` | `~/.local/bin` | Where to put the binary    |
| `BREO_VERSION`     | latest         | Version tag to install     |

### With cargo-binstall

```bash
cargo binstall breo
```

### From source

```bash
cargo install --path .
```

Then set up shell completions manually (see [Shell Completion](#shell-completion)).

## Quick Start

```bash
# Send a message (auto-creates a conversation)
breo "What is Rust's ownership model?"

# Create a named conversation
breo new rust-questions

# Send to a specific conversation
breo -c rust-questions "Explain lifetimes"

# Pipe from stdin
cat prompt.txt | breo

# Attach files for context
breo -f src/main.rs "Review this code"
```

## Commands

| Command                           | Description                                          |
| --------------------------------- | ---------------------------------------------------- |
| `breo <message>`                  | Send a message to the active conversation            |
| `breo new <name>`                 | Create a new conversation and switch to it           |
| `breo list`                       | List all conversations for the current directory     |
| `breo pick`                       | Fuzzy-pick a conversation (for shell integration)    |
| `breo rename <old> <new>`         | Rename a conversation                                |
| `breo status`                     | Show directory, conversation, agent, model, sandbox  |
| `breo compact [name]`             | Summarize a conversation to save context             |
| `breo setup <shell>`              | Print shell setup for TAB completion                 |
| `breo loop <plan> <verification>` | Run an implement/validate loop                       |
| `breo claws <bot>`                | Start a Discord bridge for a named bot profile       |
| `breo claws list`                 | List configured bot profiles                         |

## Options

| Flag                        | Description                                      |
| --------------------------- | ------------------------------------------------ |
| `-c, --conversation <name>` | Target a specific conversation and switch to it |
| `-m, --model <model>`       | Model to use (see [Models](#models))             |
| `-a, --agent <backend>`     | Backend: `claude`, `codex`, or `gemini`          |
| `-f, --files <path>...`     | Attach files to the prompt                       |
| `-s, --sandbox <name>`      | Lima VM instance name                            |
| `--no-sandbox`              | Disable sandbox mode                             |
| `--no-push`                 | Disable auto-push after commit                   |

## Backends and Models

breo dispatches to different LLM CLI tools depending on the selected backend:

| Backend    | Command                  | How prompts are sent |
| ---------- | ------------------------ | -------------------- |
| **Claude** | `claude --print`         | stdin                |
| **Codex**  | `codex exec --full-auto` | CLI argument         |
| **Gemini** | `gemini --yolo`          | CLI argument         |

### Models

| Model                      | Backend | Context Window |
| -------------------------- | ------- | -------------- |
| `sonnet`                   | Claude  | 200K           |
| `opus`                     | Claude  | 200K           |
| `haiku`                    | Claude  | 200K           |
| `gpt-5`                    | Codex   | 400K           |
| `gpt-5-mini`               | Codex   | 400K           |
| `o3`                       | Codex   | 200K           |
| `o4-mini`                  | Codex   | 200K           |
| `gemini-3.1-pro-preview`   | Gemini  | 1M             |
| `gemini-3-pro-preview`     | Gemini  | 1M             |
| `gemini-3-flash-preview`   | Gemini  | 1M             |
| `gemini-2.5-pro`           | Gemini  | 1M             |
| `gemini-2.5-flash`         | Gemini  | 1M             |

## Conversations

Conversations are plain markdown files stored under
`~/.config/breo/conversations/`. Each working directory gets its own subfolder,
so conversations are scoped to the project you're working in.

```text
~/.config/breo/
  config.toml
  state.toml
  .git/
  conversations/
    my-project/
      rust-questions.md
      debugging-session.md
    another-project/
      feature-design.md
```

Conversation files use a simple format:

```markdown
# Conversation: rust-questions

## User

What is Rust's ownership model?

## Assistant

Rust's ownership model is...
```

### Context Tracking

breo estimates token usage and displays context utilization after each message,
including exchange count, tokens used, tokens remaining, and whether the
conversation is committed to git.

### Compacting

When a conversation grows large, compact it to free context space:

```bash
breo compact              # compact the active conversation
breo compact rust-questions  # compact a specific one
```

This uses Claude to summarize the conversation, preserving key decisions, code
snippets, and current state while reducing token count.

## Git Integration

All conversations are automatically version-controlled in a git repository at
`~/.config/breo/`. Every message, new conversation, and compaction triggers a
commit. Auto-push is enabled by default and can be disabled with `--no-push` or
in the config.

## Sandbox Mode

breo can run LLM backends inside Lima VMs for isolation:

```bash
# Use the default sandbox
breo "Generate and run a script"

# Use a specific VM
breo -s my-vm "Generate and run a script"

# Disable sandbox for this command
breo --no-sandbox "Just answer a question"
```

Requires [Lima](https://lima-vm.io/) with the backend CLI tools installed inside the VM.

## Discord Bridge (claws)

The `claws` command connects breo to Discord as a bot. You can chat with breo
via DMs or @mentions in channels. Multiple bot profiles are supported.

```bash
# List configured bots
breo claws list

# Start a bot
breo claws mybot

# Start with overrides
breo claws mybot --agent gemini --model gemini-3-pro-preview --sandbox my-vm

# Set response destination (channel ID or "dm")
breo claws mybot -d 1234567890
```

### Bot Commands

Once the bot is running, send these commands in Discord:

| Command              | Description                                |
| -------------------- | ------------------------------------------ |
| `!switch <name>`     | Switch to a different conversation         |
| `!new <name>`        | Create a new conversation and switch to it |
| `!list`              | List conversations (active marked with *)  |
| `!status`            | Show bot, directory, conversation, agent, model, sandbox, destination |
| `!agent <name>`      | Switch backend (claude, codex, gemini)     |
| `!model <name>`      | Switch model                               |
| `!destination <target>` | Set response destination (channel ID or "dm") |
| `!compact`           | Compact the active conversation            |

All commands require the user to be in the bot's `allowed_users` list.
Long responses are automatically split into multiple messages (2000 char limit).

### Cron Scheduling

The bot polls `.breo/cron.toml` every 10 seconds for scheduled tasks. Tasks are
messages sent to breo on a schedule, with responses delivered to the bot's
configured destination.

```toml
# One-shot task (runs once, then removed)
[[task]]
name = "one-shot-reminder"
message = "Summarize the current state of the feature branch"
next_run = 2026-02-24T15:30:00
status = "pending"

# Periodic task (runs every 24 hours)
[[task]]
name = "daily-status"
message = "Check the logs and report any errors from the last 24 hours"
next_run = 2026-02-24T09:00:00
interval = "24h"
status = "pending"
```

The cron file is auto-created with a comment header when the bot starts, so LLM
agents can read the format and manage tasks autonomously.

## Loop Mode

The `loop` command runs an automated implement/validate cycle, useful for
agentic coding workflows:

```bash
breo loop PLAN.md VERIFICATION.md
```

- **PLAN.md** contains instructions for the implementer agent
- **VERIFICATION.md** contains validation criteria for the reviewer agent
- A `RESULT.md` file is created in the current directory as the communication
  channel between agents

The loop repeats until the validator returns `VERDICT: SUCCESS`:

```text
Implementer reads PLAN.md -> executes -> updates RESULT.md
    |
Validator reviews RESULT.md against VERIFICATION.md -> verdict
    |
    +-- SUCCESS: done
    +-- RETRY: implementer reads feedback from RESULT.md, tries again
```

Options for loop:

| Flag             | Description                                       |
| ---------------- | ------------------------------------------------- |
| `--agent`        | Backend for the implementer                       |
| `--review-agent` | Backend for the validator (defaults to `--agent`) |
| `--review-model` | Model for the validator (defaults to `--model`)   |
| `-f, --files`    | Files to attach to the implementer prompt         |

## Shell Completion

Set up fuzzy TAB completion with skim:

```bash
# Bash - add to ~/.bashrc
eval "$(breo setup bash)"

# Zsh - add to ~/.zshrc
eval "$(breo setup zsh)"

# Fish - add to ~/.config/fish/config.fish
breo setup fish | source
```

This provides fuzzy-matching conversation names when using `-c` or `compact`.

## Configuration

Config file: `~/.config/breo/config.toml`

```toml
# Default backend (claude, codex, gemini)
agent = "claude"

# Sandbox settings
sandbox = true
sandbox_name = "default"

# Auto-push after commits
push = true

# Discord bot profiles (for `breo claws`)
[discord.bots.mybot]
bot_token = "YOUR_DISCORD_BOT_TOKEN"
guild_id = "OPTIONAL_GUILD_ID"
allowed_users = ["YOUR_DISCORD_USER_ID"]

# Multiple bots supported
[discord.bots.another]
bot_token = "ANOTHER_BOT_TOKEN"
allowed_users = ["USER_ID_1", "USER_ID_2"]
```

### Resolution Cascades

Settings are resolved in order of precedence:

| Setting     | CLI flag       | Per-directory state | Global config   |
| ----------- | -------------- | ------------------- | --------------- |
| Backend     | `--agent`      | last used agent     | `agent`         |
| Model       | `--model`      | last used model     | backend default |
| Sandbox     | `--sandbox`    | last used sandbox   | `sandbox_name`  |
| Destination | `-d`           | last set destination| DM (default)    |

Per-directory state (active conversation, agent, model, sandbox, destination) is
persisted in `~/.config/breo/state.toml` and carries across invocations.
