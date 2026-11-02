# Homebase (`hb`) guide for AI agents

`hb` runs and manages Claude Code Remote Control servers on this machine: one
`claude remote-control` process per project folder, kept running by the Homebase
agent (`hbd`). A running server makes its folder reachable from the Claude mobile
app and claude.ai/code under the server's name.

Print this guide any time with `hb guide`.

## Rules for agents

1. Pass `--json` on every command and read stdout. Errors come back on stdout too,
   as `{"ok": false, "error": {"code": "...", "message": "..."}}`.
2. Check the exit code: `0` ok, `1` error, `2` bad usage or invalid input,
   `3` server not found.
3. `hb` never prompts. It refuses what needs a person and says why.
4. Never answer claude's prompts for the user, and never read or copy anything in
   `~/.claude` or the keychain. Sign-in and folder trust belong to the person.
5. If you are running inside a Remote Control session, `HOMEBASE_SERVER_ID` names
   the server hosting you. Stopping, restarting or removing it ends your session.
   `hb` refuses without `--yes`. Say so to the user before you pass it, and do it
   last.
6. Folders are never deleted or changed by `hb`. `hb rm` only forgets the server.

## Set up a Mac (macOS 14 or later)

```sh
hb doctor --json                      # 1. what's missing
hb add ~/Projects/my-app --start --json   # 2. one server per project folder
hb ls --json                          # 3. confirm state is "running"
hb service install --json             # 4. headless Mac: start at login
```

What `hb doctor` checks and how each one is fixed:

| Check | Means | Fix (who) |
|---|---|---|
| `claude` | Claude Code isn't installed or isn't on the PATH | Install from https://code.claude.com, or set `claudePath` in servers.json (agent or person) |
| `sign-in` | Not signed in, or not with a Pro, Max, Ultra, Team or Enterprise plan | The person runs `claude auth login` in a terminal |
| `environment` | `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL` or a Bedrock, Vertex or Foundry variable is set | Unset it where the agent runs. Remote Control needs a subscription sign-in |
| `agent` | `hbd` isn't running | Any `hb` command starts it. `hb service install` starts it at login |
| `folder` | A server's folder is gone | `hb edit <id> --folder <path>` or `hb rm <id>` |
| `server` | A server needs attention or failed | See "Needs attention" below |
| `other servers` | `claude remote-control` is running outside Homebase (tmux, a terminal) | Stop those, then `hb add` the folders |
| `sleep` | The Mac sleeps when idle. Servers drop about 10 min after sleep | Headless Mac: `sudo pmset -a sleep 0 && sudo pmset -a womp 1` (the person, needs sudo) |

A headless Mac also needs a logged-in user. Turn on automatic login, because the
agent runs in that user's login session. Sign `claude` in once over SSH or Screen
Sharing.

## Windows

Not supported yet. The agent and CLI are written to port, and Windows is a
planned milestone. Until then, run `claude remote-control` by hand on Windows.

## Commands

| Command | Does | `--json` stdout |
|---|---|---|
| `hb ls` | Every server and its state | `[ServerStatus]` |
| `hb status <id>` | One server, including the exact command line and last output | `ServerStatus` |
| `hb add <folder> [opts] [--start]` | Creates a server. The name defaults to the folder name, the id to a slug of the name | `Server` |
| `hb edit <id> [opts] [--clear FIELD] [--no-restart]` | Changes options. Restarts a running server unless `--no-restart` | `Server` |
| `hb rm <id> [--yes]` | Stops and forgets a server | `Server` |
| `hb start [<id>...\|--all]` | No arguments starts the `autoStart` servers | `[ActionResult]` |
| `hb stop <id>...\|--all [--yes]` | SIGINT, then SIGTERM after 10 s | `[ActionResult]` |
| `hb restart <id>...\|--all [--yes]` | Returns as soon as the agent has the request | `[ActionResult]` |
| `hb config [<id>]` | The stored config | servers.json content |
| `hb logs <id> [-n N] [-f]` | A server's output, ANSI removed | `{"id","logFile","lines":[...]}`. With `-f`, one `{"id","line"}` per line |
| `hb doctor` | The checks above | `{"ok": bool, "checks": [{"name","status","message","fix"?}]}` |
| `hb service install\|uninstall\|status` | The LaunchAgent that starts `hbd` at login | `{"installed","loaded",...}` |
| `hb skill install [--project] [--force]` | Writes the Claude Code skill for `hb` | `{"path","changed"}` |

Options for `add` and `edit` map one to one to `claude remote-control` flags:

| hb option | claude flag | Default for new servers |
|---|---|---|
| `--name` | `--name` | the folder's name |
| `--spawn auto\|same-dir\|worktree\|session` | `--spawn` | `auto`: `worktree` for a git repo with a commit, else `same-dir` |
| `--capacity N` | `--capacity` | claude's default |
| `--permission-mode M` | `--permission-mode` | claude's default |
| `--create-session` / `--no-create-session` | `--[no-]create-session-in-dir` | `--no-create-session` |
| `--chrome` / `--no-chrome` | `--[no-]chrome` | claude's default |
| `--name-prefix P` | `--remote-control-session-name-prefix` | claude's default |
| `--env KEY=VALUE` (repeatable) | environment | none |
| `-- <args>` | appended as-is | none |
| `--restart never\|on-failure\|always` | (Homebase) | `on-failure` |
| `--no-autostart` | (Homebase) | autostart on |

## JSON shapes

`ServerStatus`:

```json
{
  "id": "my-app",
  "name": "my-app",
  "folder": "~/Projects/my-app",
  "state": "running",
  "pid": 41213,
  "startedAt": 1793620000,
  "restarts": 0,
  "lastExit": null,
  "attention": null,
  "command": "/Users/me/.local/bin/claude remote-control --name my-app --spawn worktree --no-create-session-in-dir",
  "lastLines": ["..."],
  "logFile": "/Users/me/Library/Logs/Homebase/my-app.log"
}
```

`state` is one of `stopped`, `starting` (the first 3 s), `running`,
`needs-attention`, `failed` or `backing-off`. `command` appears in `hb status`
and is `null` in `hb ls` for stopped servers.

`ActionResult`: `{"id": "my-app", "ok": true}`, or with `"ok": false` and a
`"message"`. The command exits 1 if any result failed.

## Needs attention

`attention` says why. Two cases are common:

- **Folder trust.** claude asks whether to trust a folder the first time it runs
  there. The person runs `cd <folder> && claude`, accepts, exits, and then you run
  `hb restart <id>`. Tell the user. Don't try to answer it yourself.
- **Sign-in.** The person runs `claude auth login`, then you run
  `hb restart <id>`.

For anything else, read `hb logs <id> -n 50 --json` and `hb status <id> --json`
(`lastLines`, `lastExit`).

## Files

| What | Where |
|---|---|
| Server config (people may edit it by hand, and the agent picks it up) | `~/.config/homebase/servers.json` |
| Logs (rotated at 5 MB, 3 kept) | `~/Library/Logs/Homebase/<id>.log`, and the agent's own log `hbd.log` |
| Control socket (mode 0600, one JSON object per line) | `~/Library/Application Support/Homebase/hbd.sock` |

Homebase isn't affiliated with Anthropic. It runs your own, unmodified `claude`.
It never signs in for you or touches your credentials.
