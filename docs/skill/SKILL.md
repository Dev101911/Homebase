---
name: homebase
description: Manage Claude Code Remote Control servers on this machine with the Homebase `hb` CLI. Use when asked to make a project reachable from the Claude app or claude.ai/code, to set up, list, start, stop or restart Remote Control (RC) servers, to read a server's log, or to find out why one isn't running.
---

# Homebase (`hb`)

`hb` keeps one `claude remote-control` server per project folder running on this
machine.

Before anything else, run `hb guide`. It is the full reference: commands, JSON
shapes, exit codes and fixes.

Essentials:

- Always pass `--json`. Exit codes: 0 ok, 1 error, 2 bad usage, 3 not found.
- Start with `hb doctor --json`, then `hb ls --json`.
- Add a project: `hb add <folder> --start --json`. Then check
  `hb status <id> --json`.
- `needs-attention` means a person must act: the folder-trust question
  (`cd <folder> && claude`, accept, then `hb restart <id>`) or sign-in
  (`claude auth login`). Tell the user. Never answer prompts yourself, and never
  read `~/.claude` credentials.
- `HOMEBASE_SERVER_ID` names the server hosting this session. Stopping,
  restarting or removing it ends the session. Warn the user, then pass `--yes`,
  and do it last.
