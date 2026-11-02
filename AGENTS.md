# Homebase: agent guide

Instructions for AI coding agents working on this repo (Claude Code, Codex,
Gemini and others). For using `hb` rather than changing it, read
`docs/agent-guide.md`.

## Project shape

- `crates/homebase-core`: the server model and `servers.json`, the socket
  protocol, finding `claude`, building its command line, ANSI stripping, and
  the prompt patterns (`prompts.rs`, the only place they live).
- `crates/hbd`: the agent. Supervises each `claude remote-control` in a PTY,
  writes logs and serves the Unix socket.
- `crates/hb`: the CLI. `docs/agent-guide.md` and `docs/skill/SKILL.md` are
  compiled into it (`hb guide`, `hb skill`). Keep them in step with the CLI.
- `app/`: the SwiftUI menu bar app (a Swift package, macOS 14+). It talks to
  hbd over the socket and holds no supervision logic. `Scripts/build-app.sh`
  bundles it with `hb` and `hbd` into `build/Homebase.app`.
- The design is the "Homebase — Design" doc. Milestones there set the build order.

Hard rules from the design: never bundle or modify `claude`; never read
`~/.claude` credentials or keychain items; never answer claude's prompts; never
put flags before `remote-control`; keep "Claude" out of the product name and icon.

## Conventions

Every task links to an issue, branches are `issue-<n>-slug`, Conventional
Commits, no force-pushes, no pushes to `main`, change only what was asked, and
don't commit or push unless asked.

Commit scopes: `core`, `agent`, `cli`, `app`, `docs`. Leave the scope out
rather than inventing one.

## Checks before a PR

There's no CI. Before opening a PR:

- `cargo build --workspace` with zero warnings.
- `cargo clippy --workspace --all-targets` with zero warnings.
- `cargo test --workspace` passes. The end-to-end tests in
  `crates/hb/tests/cli.rs` run a fake `claude` and need the whole workspace built.
- `swift build --package-path app` with zero warnings, and
  `swift test --package-path app` passes.
- `Scripts/build-app.sh` builds and signs the app.
- If the CLI changed, `docs/agent-guide.md`, `docs/skill/SKILL.md` and
  `README.md` say the same thing.
