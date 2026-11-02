//! `hb doctor`: why a server would or wouldn't work on this machine.
//! It never reads claude's credentials: sign-in comes from `claude auth status`.

use crate::output::{Failure, Out};
use homebase_core::protocol::{ServerState, ServerStatus};
use homebase_core::{Config, Paths, Request, claude, client};
use serde::Serialize;
use std::process::{Command, Stdio};

#[derive(Serialize)]
struct Check {
    name: &'static str,
    status: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<String>,
}

fn ok(name: &'static str, message: impl Into<String>) -> Check {
    Check {
        name,
        status: "ok",
        message: message.into(),
        fix: None,
    }
}
fn warn(name: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        name,
        status: "warn",
        message: message.into(),
        fix: Some(fix.into()),
    }
}
fn fail(name: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Check {
    Check {
        name,
        status: "fail",
        message: message.into(),
        fix: Some(fix.into()),
    }
}

pub fn run(paths: &Paths, out: &Out) -> Result<(), Failure> {
    let mut checks = Vec::new();

    let config = match Config::load(&paths.servers_file()) {
        Ok(c) => {
            checks.push(ok(
                "config",
                format!(
                    "{} ({} servers)",
                    paths.servers_file().display(),
                    c.servers.len()
                ),
            ));
            Some(c)
        }
        Err(e) => {
            checks.push(fail(
                "config",
                e.to_string(),
                format!("fix or delete {}", paths.servers_file().display()),
            ));
            None
        }
    };

    let claude_path = config.as_ref().and_then(|c| c.claude_path.clone());
    match claude::find_claude(claude_path.as_deref()) {
        None => checks.push(fail(
            "claude",
            "claude not found",
            "install Claude Code (https://code.claude.com), or set claudePath in servers.json",
        )),
        Some(path) => {
            let version = Command::new(&path)
                .arg("--version")
                .stdin(Stdio::null())
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            checks.push(ok("claude", format!("{} ({version})", path.display())));
            checks.push(auth_check(&path));
        }
    }

    let env_problems: Vec<&str> = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_BASE_URL",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ]
    .into_iter()
    .filter(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
    .collect();
    if env_problems.is_empty() {
        checks.push(ok(
            "environment",
            "no API-key or third-party provider variables set",
        ));
    } else {
        checks.push(warn(
            "environment",
            format!(
                "{} set; Remote Control needs a claude.ai subscription sign-in",
                env_problems.join(", ")
            ),
            "unset them in the environment the agent runs in",
        ));
    }

    let agent_servers: Option<Vec<ServerStatus>> =
        match client::request(&paths.socket(), &Request::List) {
            Ok(r) if r.ok => {
                checks.push(ok(
                    "agent",
                    format!("running ({})", paths.socket().display()),
                ));
                r.data.and_then(|d| serde_json::from_value(d).ok())
            }
            _ => {
                checks.push(warn(
                    "agent",
                    "not running",
                    "any hb command starts it; `hb service install` starts it at login",
                ));
                None
            }
        };

    if let Some(c) = &config {
        for s in &c.servers {
            if !s.folder_path().is_dir() {
                checks.push(fail(
                    "folder",
                    format!("{}: {} doesn't exist", s.id, s.folder),
                    format!("`hb edit {} --folder <path>` or `hb rm {}`", s.id, s.id),
                ));
            }
        }
    }
    if let Some(list) = &agent_servers {
        for s in list {
            match s.state {
                ServerState::NeedsAttention => checks.push(warn(
                    "server",
                    format!("{}: {}", s.id, s.attention.clone().unwrap_or_default()),
                    format!(
                        "cd {} && claude   (answer the question, exit, then `hb restart {}`)",
                        s.folder, s.id
                    ),
                )),
                ServerState::Failed => checks.push(fail(
                    "server",
                    format!(
                        "{}: {}",
                        s.id,
                        s.attention.clone().unwrap_or_else(|| "failed".into())
                    ),
                    format!("`hb logs {}` to see why, then `hb start {}`", s.id, s.id),
                )),
                _ => {}
            }
        }
    }

    let managed: Vec<u32> = agent_servers
        .iter()
        .flatten()
        .filter_map(|s| s.pid)
        .collect();
    let strays = stray_servers(&managed);
    if strays > 0 {
        checks.push(warn(
            "other servers",
            format!("{strays} claude remote-control process(es) not run by Homebase (tmux or a terminal?)"),
            "stop them (e.g. `tmux kill-server`) and add their folders with `hb add`, so each folder has one server",
        ));
    }

    if cfg!(target_os = "macos")
        && let Some(minutes) = sleep_minutes()
    {
        if minutes == 0 {
            checks.push(ok("sleep", "the Mac never sleeps on its own"));
        } else {
            checks.push(warn(
                "sleep",
                format!("the Mac sleeps after {minutes} min idle; servers go offline after about 10 min asleep"),
                "on a headless Mac: sudo pmset -a sleep 0 && sudo pmset -a womp 1",
            ));
        }
    }

    let failed = checks.iter().any(|c| c.status == "fail");
    if out.json {
        out.print_json(&serde_json::json!({ "ok": !failed, "checks": checks }));
    } else {
        for c in &checks {
            let mark = match c.status {
                "ok" => "✓",
                "warn" => "!",
                _ => "✗",
            };
            println!("{mark} {}: {}", c.name, c.message);
            if let Some(fix) = &c.fix {
                println!("    fix: {fix}");
            }
        }
    }
    if failed {
        let mut f = Failure::error("some checks failed");
        f.silent = true;
        return Err(f);
    }
    Ok(())
}

fn auth_check(claude: &std::path::Path) -> Check {
    let output = Command::new(claude)
        .args(["auth", "status"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return warn(
            "sign-in",
            "couldn't run `claude auth status`",
            "run `claude auth status` yourself",
        );
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return warn(
            "sign-in",
            "couldn't read `claude auth status`",
            "run `claude auth status` yourself",
        );
    };
    if v["loggedIn"] != true {
        return fail("sign-in", "claude isn't signed in", "claude auth login");
    }
    let method = v["authMethod"].as_str().unwrap_or("unknown");
    if method != "claude.ai" {
        return fail(
            "sign-in",
            format!("signed in with {method}; Remote Control needs a claude.ai subscription"),
            "claude auth login (choose your Claude account)",
        );
    }
    let plan = v["subscriptionType"].as_str().unwrap_or("unknown");
    let paid = ["pro", "max", "ultra", "team", "enterprise"];
    if paid.contains(&plan) {
        ok("sign-in", format!("signed in with a claude.ai {plan} plan"))
    } else {
        warn(
            "sign-in",
            format!("plan '{plan}'; Remote Control needs Pro, Max, Ultra, Team or Enterprise"),
            "upgrade the plan, or sign in with another account",
        )
    }
}

/// Counts `claude remote-control` processes the agent doesn't own.
fn stray_servers(managed: &[u32]) -> usize {
    let Ok(out) = Command::new("ps").args(["-axo", "pid=,command="]).output() else {
        return 0;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, cmd) = l.split_once(' ')?;
            let pid: u32 = pid.parse().ok()?;
            let is_server = cmd.contains("remote-control")
                && cmd
                    .split_whitespace()
                    .next()
                    .is_some_and(|p| p.ends_with("claude"));
            (is_server && !managed.contains(&pid)).then_some(pid)
        })
        .count()
}

fn sleep_minutes() -> Option<u32> {
    let out = Command::new("pmset").arg("-g").output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        (parts.next()? == "sleep").then(|| parts.next()?.parse().ok())?
    })
}
