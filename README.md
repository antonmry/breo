# breo

A CLI tool for working with code agents.

breo orchestrates LLM-powered code agents, storing all interactions as versioned
markdown files. It supports multiple backends (Claude, Codex, Gemini), runs
agents in sandboxed Lima VMs, scopes conversations per project directory, and
provides an automated implement/validate loop for agentic coding workflows.

## Installation

```bash
cargo install --path .
```

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

| Command                      | Description                                       |
| ---------------------------- | ------------------------------------------------- |
| `breo <message>`             | Send a message to the active conversation         |
| `breo new <name>`            | Create a new conversation and switch to it        |
| `breo list`                  | List all conversations for the current directory  |
| `breo pick`                  | Fuzzy-pick a conversation (for shell integration) |
| `breo status`                | Show active conversation, agent, and sandbox      |
| `breo compact [name]`        | Summarize a conversation to save context          |
| `breo setup <shell>`         | Print shell setup for TAB completion              |
| `breo loop <plan> <harness>` | Run an implement/validate loop                    |

## Options

| Flag                        | Description                                      |
| --------------------------- | ------------------------------------------------ |
| `-c, --conversation <name>` | Target a specific conversation without switching |
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

| Model              | Backend | Context Window |
| ------------------ | ------- | -------------- |
| `sonnet`           | Claude  | 200K           |
| `opus`             | Claude  | 200K           |
| `haiku`            | Claude  | 200K           |
| `gpt-5`            | Codex   | 400K           |
| `gpt-5-mini`       | Codex   | 400K           |
| `o3`               | Codex   | 200K           |
| `o4-mini`          | Codex   | 200K           |
| `gemini-2.5-pro`   | Gemini  | 1M             |
| `gemini-2.5-flash` | Gemini  | 1M             |

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

## Loop Mode

The `loop` command runs an automated implement/validate cycle, useful for
agentic coding workflows:

```bash
breo loop PLAN.md HARNESS.md
```

- **PLAN.md** contains instructions for the implementer agent
- **HARNESS.md** contains validation criteria for the reviewer agent
- A `RESULT.md` file is created in the current directory as the communication
  channel between agents

The loop repeats until the validator returns `VERDICT: SUCCESS`:

```text
Implementer reads PLAN.md -> executes -> updates RESULT.md
    |
Validator reviews RESULT.md against HARNESS.md -> verdict
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
```

All config values can be overridden per-command with CLI flags. Per-directory
state (active conversation, agent, sandbox) is persisted in
`~/.config/breo/state.toml`.
