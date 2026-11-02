# Homebase

Homebase keeps your projects reachable from the Claude mobile app and
claude.ai/code. It runs one Claude Code Remote Control server
(`claude remote-control`) per project folder and keeps them running, without
the Claude desktop app and without a pile of tmux sessions.

- **`hb`**: a command line tool for people, scripts and AI agents.
- **`hbd`**: a small background agent that owns the server processes.
- **A macOS menu bar app**: the same controls at a glance, with a Settings window
  and a log viewer. It's a client of the agent, so quitting it leaves your servers running.

Homebase isn't affiliated with Anthropic. It runs your own, unmodified `claude`.
It never bundles Claude Code, never signs in for you and never touches your
credentials.

> **Status:** early. The core, agent, CLI and a first menu bar app work on
> macOS. See [Roadmap](#roadmap).

## Requirements

- macOS 14 Sonoma or later. Windows is planned.
- [Claude Code](https://code.claude.com), installed and signed in with a Pro,
  Max, Ultra, Team or Enterprise plan. API keys, Bedrock, Vertex and Foundry
  don't support Remote Control.

## Install (from source, for now)

You need Rust (`brew install rustup && rustup default stable`) and Xcode 16 or later.

```sh
git clone https://github.com/Dev101911/Homebase.git
cd Homebase
Scripts/build-app.sh --open      # builds build/Homebase.app with hb and hbd inside
```

Then use **Settings → General → Install the hb Command** to link `hb` into
`~/.local/bin`. For the CLI alone (a headless Mac), `cargo build --release` and
link `target/release/hb`. `hbd` is found next to `hb`.

Signed DMG, Homebrew cask and formula come with the 1.0 release.

## The menu bar app

- **Popover:** one row per server with its state, uptime and folder, plus the
  obvious next action: Start, Stop or Fix. Right-click a row for more.
- **Settings:** a sidebar of servers and a form for each one. Every field maps
  to a `claude remote-control` flag, and "Command that runs" shows the exact
  command line. General holds start at login, the `hb` command and diagnostics.
- **Log window:** follows a server's log, with Copy and Reveal in Finder.
- **Fix:** opens Terminal in the server's folder, so you can answer claude's
  trust question yourself. Homebase never answers it for you.

## Quick start

```sh
hb doctor                                   # is claude installed and signed in?
hb add ~/Projects/my-app --start            # serve a folder
hb ls                                       # every server and its state
hb logs my-app -f                           # follow its output
hb service install                          # start the agent at login
```

The first time `claude` runs in a folder it asks whether you trust it. Homebase
never answers that for you. The server shows **needs-attention**. Run
`cd <folder> && claude` once, accept, exit, then `hb restart <id>`.

## Let an AI agent set it up

`hb` is built to be driven by agents. Every command takes `--json`, errors come
back as JSON, exit codes are stable (0 ok, 1 error, 2 bad usage, 3 not found),
and nothing prompts.

- `hb guide` prints the full agent reference: commands, JSON shapes and fixes.
  It's also in [docs/agent-guide.md](docs/agent-guide.md).
- `hb skill install` adds a Claude Code skill, so any Claude Code session on
  the machine knows how to use `hb` safely.

To set up a new Mac, paste this into Claude Code on it:

> Install Homebase from https://github.com/Dev101911/Homebase, then run
> `hb guide` and follow it to serve each folder in ~/Projects with Remote
> Control. Run `hb doctor` and tell me anything I need to do myself.

You can also do it from your phone. A Remote Control session on the Mac can run
`hb`, so you can ask it to "restart the my-app server" or "show me my-app's
log". `hb` warns, and needs `--yes`, before stopping the server that hosts the
session you're typing in.

## Commands

| Command | What it does |
|---|---|
| `hb ls` | List servers: id, name, state, uptime, folder |
| `hb add <folder> [options] [--start]` | Create a server |
| `hb edit <id> [options]` | Change options (restarts it if running) |
| `hb rm <id>` | Stop and forget a server. The folder is never touched |
| `hb start [<id>...\|--all]` | Start servers. With no arguments, the autostart ones |
| `hb stop <id>...\|--all` | Stop servers (SIGINT, then SIGTERM after 10 s) |
| `hb restart <id>...\|--all` | Stop, then start |
| `hb status <id>` | State, pid, restarts, last exit and the exact command line |
| `hb config [<id>]` | The stored config as JSON |
| `hb logs <id> [-f] [-n 100]` | Print or follow a server's log |
| `hb doctor` | Check claude, sign-in, the agent, folders and sleep settings |
| `hb service install\|uninstall\|status` | Start the agent at login (LaunchAgent) |
| `hb guide`, `hb skill install` | Docs for AI agents |

Server options map one to one to `claude remote-control` flags: `--spawn`,
`--capacity`, `--permission-mode`, `--[no-]create-session`, `--[no-]chrome`,
`--name-prefix`, `--env KEY=VALUE`, and anything after `--`.

## Files

| What | Where |
|---|---|
| Server config (edit it by hand if you like) | `~/.config/homebase/servers.json` |
| Logs | `~/Library/Logs/Homebase/<id>.log` |
| Control socket | `~/Library/Application Support/Homebase/hbd.sock` |

## Roadmap

1. **Core + CLI** (this release): `homebase-core`, `hbd` and `hb`.
2. **Reliability:** restart with backoff, restart on wake and network return,
   keep-awake, start at login from the app.
3. **Menu bar app** (first version in): SwiftUI popover, Settings window, log viewer.
4. **Public 1.0:** signed and notarized DMG, Sparkle updates, Homebrew.
5. **Later:** Windows (agent, CLI, then a tray app), tmux import, health
   checks, notifications and groups.

## Licence

[Apache-2.0](LICENSE). © 2026 TulipByte Studio.
