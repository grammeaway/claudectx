# claudectx 

Easily switch between multiple Claude Code contexts.

Use an account for work, but another for personal use? No problem. With claudectx, you can quickly switch between different Claude Code contexts, all from the comfort of your terminal.

Inspired by `kubectx`.

```
$ claudectx list
✓ work
  personal
  client-acme

$ claudectx personal
✓ Switched to context personal (~/.claude.json, ~/.claude/settings.json)
  Restart Claude Code for changes to take effect.
```

## Why?

Claude Code stores your identity, OAuth session, MCP servers, and settings in two files:

| File | Contains |
|------|----------|
| `~/.claude.json` | OAuth session, account, MCP server configs, preferences (theme, editor mode) |
| `~/.claude/settings.json` | Model selection, tool permissions, env vars, hooks |

If you use Claude Code with a work account **and** a personal account (or multiple API keys / subscriptions), swapping between them means manually editing those files each time. `claudectx` automates that.

## Installation

### From source (requires Rust / cargo)

```bash
git clone <this-repo>
cd claudectx
cargo build --release
# copy binary somewhere on your PATH
cp target/release/claudectx /usr/local/bin/
```

### Pre-built binary

Download from the releases page and place on your `$PATH`.

## Usage

```
claudectx [CONTEXT] [COMMAND]
```

### Quick switch (kubectx style)

```bash
claudectx work        # switch to the "work" context
claudectx personal    # switch to the "personal" context
```

### Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `claudectx` | | List all contexts (default when no args) |
| `claudectx list` | `ls` | List all contexts |
| `claudectx save <name>` | | Snapshot current config as a named context |
| `claudectx use <name>` | | Restore a saved context (same as bare `claudectx <name>`) |
| `claudectx current` | | Print the active context name |
| `claudectx delete <name>` | `rm` | Delete a saved context |
| `claudectx rename <old> <new>` | | Rename a context |
| `claudectx copy <src> <dst>` | | Duplicate a context |
| `claudectx inspect <name>` | | Show files + top-level JSON keys stored in a context |

### Typical workflow

```bash
# 1. Log into your work account in Claude Code, configure it how you like
claudectx save work

# 2. Log into your personal account, set it up
claudectx save personal

# 3. From now on, switching is instant
claudectx work
claudectx personal
```

### Shell prompt integration

Show the active context in your prompt (bash/zsh):

```bash
# Add to ~/.bashrc or ~/.zshrc
claudectx_prompt() {
  local ctx
  ctx=$(claudectx current 2>/dev/null)
  [[ -n "$ctx" ]] && echo "(%ctx%)" | sed "s/%ctx%/$ctx/"
}
PS1='$(claudectx_prompt) \$ '
```

## How it works

Contexts are stored in `~/.claudectx/`:

```
~/.claudectx/
├── current              ← name of active context
└── contexts/
    ├── work/
    │   ├── claude.json       ← snapshot of ~/.claude.json
    │   └── settings.json     ← snapshot of ~/.claude/settings.json
    └── personal/
        ├── claude.json
        └── settings.json
```

When you **save** a context, both files are copied in. When you **use** a context, they are copied back out (the previous live files are backed up as `.bak` first, just in case).

## Tips

- **Partial contexts are fine.** If you only have `~/.claude.json` and no `settings.json`, that's saved and restored correctly.
- **MCP servers per-account.** Since `~/.claude.json` stores user-scoped MCP servers, switching context also switches which MCP servers are active.
- **Backups.** Before overwriting your live files, the previous versions are saved as `~/.claude.json.bak` and `~/.claude/settings.json.bak`.
- **Restart Claude Code** after switching — it reads config at startup.

## Security

Context files are stored unencrypted in `~/.claudectx/`. On macOS, OAuth tokens inside `~/.claude.json` may actually be references to Keychain entries, not raw secrets. On Linux/Windows, they may be stored in plaintext — keep `~/.claudectx/` permissions tight:

```bash
chmod 700 ~/.claudectx
```

Never commit your `~/.claudectx/` directory to version control.
