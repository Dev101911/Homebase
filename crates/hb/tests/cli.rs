//! End-to-end: hb starts hbd, which runs a fake `claude` in a PTY.
//! Needs the whole workspace built (`cargo test --workspace`), so hbd sits next to hb.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

struct Env {
    root: tempfile::TempDir,
    claude: PathBuf,
}

const FAKE_CLAUDE: &str = r#"#!/bin/sh
echo "fake claude $*"
echo "cwd=$(pwd) server=$HOMEBASE_SERVER_ID"
if [ -f "$(pwd)/ask-trust" ]; then
  printf 'Do you trust the files in this folder? [y/N] '
fi
trap 'echo got-sigint; exit 0' INT
while true; do sleep 0.1; done
"#;

impl Env {
    fn new() -> Env {
        // Short path: Unix socket paths are limited to about 100 bytes.
        let root = tempfile::Builder::new()
            .prefix("hb")
            .tempdir_in("/tmp")
            .unwrap();
        let claude = root.path().join("claude");
        std::fs::write(&claude, FAKE_CLAUDE).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(root.path().join("cfg")).unwrap();
        std::fs::write(
            root.path().join("cfg/servers.json"),
            format!(
                r#"{{"version":1,"claudePath":"{}","servers":[]}}"#,
                claude.display()
            ),
        )
        .unwrap();
        Env { root, claude }
    }

    fn project(&self, name: &str) -> PathBuf {
        let p = self.root.path().join(name);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::canonicalize(p).unwrap() // /tmp is a symlink on macOS
    }

    fn hb(&self, args: &[&str]) -> Output {
        let hb = PathBuf::from(env!("CARGO_BIN_EXE_hb"));
        let hbd = hb.parent().unwrap().join("hbd");
        assert!(
            hbd.exists(),
            "build the workspace first: {} is missing",
            hbd.display()
        );
        Command::new(&hb)
            .args(args)
            .env("HOMEBASE_CONFIG_DIR", self.root.path().join("cfg"))
            .env("HOMEBASE_DATA_DIR", self.root.path().join("d"))
            .env("HOMEBASE_LOG_DIR", self.root.path().join("logs"))
            .env("HOMEBASE_HBD", &hbd)
            .env_remove("HOMEBASE_SERVER_ID")
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> (i32, Value) {
        let mut all = args.to_vec();
        all.push("--json");
        let out = self.hb(&all);
        let text = String::from_utf8_lossy(&out.stdout);
        let value = serde_json::from_str(&text).unwrap_or_else(|e| {
            panic!(
                "hb {args:?}: not JSON ({e}): {text}\nstderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        });
        (out.status.code().unwrap_or(-1), value)
    }

    fn wait_for_state(&self, id: &str, state: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let (_, s) = self.json(&["status", id]);
            if s["state"] == state {
                return s;
            }
            assert!(Instant::now() < deadline, "{id} never reached {state}: {s}");
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn log(&self, id: &str) -> String {
        std::fs::read_to_string(self.root.path().join(format!("logs/{id}.log"))).unwrap_or_default()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.hb(&["agent", "stop"]);
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn wait_for(path: &Path, needle: &str, env: &Env, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !env.log(id).contains(needle) {
        assert!(
            Instant::now() < deadline,
            "{} never contained {needle:?}:\n{}",
            path.display(),
            env.log(id)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn full_lifecycle() {
    let env = Env::new();
    let folder = env.project("My App");
    let folder_s = folder.to_str().unwrap();

    let (code, list) = env.json(&["ls"]);
    assert_eq!(code, 0);
    assert_eq!(list, serde_json::json!([]));

    let (code, server) = env.json(&["add", folder_s, "--capacity", "3", "--env", "FOO=bar"]);
    assert_eq!(code, 0, "{server}");
    assert_eq!(server["id"], "my-app");
    assert_eq!(server["name"], "My App");
    assert_eq!(server["capacity"], 3);
    assert_eq!(server["createSessionInDir"], false);

    let (code, err) = env.json(&["add", folder_s]);
    assert_eq!(code, 2, "duplicate id is invalid: {err}");
    assert_eq!(err["ok"], false);

    let (code, _) = env.json(&["start"]);
    assert_eq!(code, 0);
    let status = env.wait_for_state("my-app", "running");
    let command = status["command"].as_str().unwrap();
    assert!(
        command.starts_with(&format!(
            "{} remote-control --name 'My App'",
            env.claude.display()
        )),
        "{command}"
    );
    assert!(
        command.contains("--spawn same-dir"),
        "not a git repo: {command}"
    );
    assert!(command.contains("--capacity 3"));
    let log_path = env.root.path().join("logs/my-app.log");
    wait_for(&log_path, "server=my-app", &env, "my-app");
    assert!(env.log("my-app").contains(&format!("cwd={folder_s}")));

    let (code, ls) = env.json(&["ls"]);
    assert_eq!(code, 0);
    assert_eq!(ls[0]["state"], "running");

    let (code, logs) = env.json(&["logs", "my-app", "-n", "5"]);
    assert_eq!(code, 0);
    assert!(
        logs["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("fake claude"))
    );

    let (code, _) = env.json(&["restart", "my-app"]);
    assert_eq!(code, 0);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let (_, s) = env.json(&["status", "my-app"]);
        if s["restarts"] == 1 && s["state"] == "running" {
            break;
        }
        assert!(Instant::now() < deadline, "restart never finished: {s}");
        std::thread::sleep(Duration::from_millis(200));
    }

    let (code, _) = env.json(&["stop", "my-app"]);
    assert_eq!(code, 0);
    env.wait_for_state("my-app", "stopped");
    assert!(
        env.log("my-app").contains("got-sigint"),
        "stopped gently with SIGINT"
    );

    let (code, edited) = env.json(&["edit", "my-app", "--clear", "capacity", "--name", "Renamed"]);
    assert_eq!(code, 0, "{edited}");
    assert_eq!(edited["capacity"], Value::Null);
    assert_eq!(edited["name"], "Renamed");

    let (code, err) = env.json(&["status", "nope"]);
    assert_eq!(code, 3);
    assert_eq!(err["error"]["code"], "not-found");

    let (code, _) = env.json(&["rm", "my-app"]);
    assert_eq!(code, 0);
    assert!(folder.exists(), "rm never touches the folder");
    let (_, list) = env.json(&["ls"]);
    assert_eq!(list, serde_json::json!([]));
}

#[test]
fn trust_prompt_needs_attention() {
    let env = Env::new();
    let folder = env.project("untrusted");
    std::fs::write(folder.join("ask-trust"), "").unwrap();
    let (code, _) = env.json(&["add", folder.to_str().unwrap(), "--start"]);
    assert_eq!(code, 0);
    let status = env.wait_for_state("untrusted", "needs-attention");
    assert!(
        status["attention"].as_str().unwrap().contains("trust"),
        "{status}"
    );
    let (code, _) = env.json(&["stop", "--all"]);
    assert_eq!(code, 0);
    env.wait_for_state("untrusted", "stopped");
}

#[test]
fn guards_the_hosting_server() {
    let env = Env::new();
    let hb = PathBuf::from(env!("CARGO_BIN_EXE_hb"));
    let out = Command::new(&hb)
        .args(["stop", "--all", "--json"])
        .env("HOMEBASE_SERVER_ID", "general")
        .env("HOMEBASE_DATA_DIR", env.root.path().join("d"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["error"]["message"].as_str().unwrap().contains("--yes"));
}

#[test]
fn guide_and_skill_are_available() {
    let env = Env::new();
    let out = env.hb(&["guide"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("# Homebase (`hb`) guide for AI agents"));
    let out = env.hb(&["skill", "print"]);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("---\nname: homebase\n"));
    let out = env.hb(&["--help"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("hb guide"));
    let out = env.hb(&["bogus"]);
    assert_eq!(out.status.code(), Some(2));
}
