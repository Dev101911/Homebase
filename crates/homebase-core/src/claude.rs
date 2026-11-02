//! Finding the user's own `claude` and building the exact command line a
//! server runs. Homebase never passes flags before `remote-control`: global
//! flags there make `claude remote-control` refuse to start.

use crate::config::{Server, SpawnMode, expand_home};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where `claude` is looked for after the login shell's PATH.
pub fn fallback_locations() -> Vec<PathBuf> {
    let home = crate::paths::home_dir();
    vec![
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
}

/// Finds `claude`: the configured path, else the login shell's PATH, else the fallbacks.
pub fn find_claude(configured: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = configured {
        let path = expand_home(p);
        return is_executable(&path).then_some(path);
    }
    if let Some(p) = from_login_shell() {
        return Some(p);
    }
    fallback_locations().into_iter().find(|p| is_executable(p))
}

fn from_login_shell() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let out = Command::new(shell)
        .args(["-l", "-c", "command -v claude"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().rev().find(|l| l.starts_with('/'))?;
    let path = PathBuf::from(line.trim());
    is_executable(&path).then_some(path)
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// True when `folder` is inside a git repo that has at least one commit.
pub fn is_git_repo_with_commit(folder: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["rev-parse", "--verify", "--quiet", "HEAD"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Turns `auto` into the mode that will actually be passed.
pub fn resolve_spawn(server: &Server) -> SpawnMode {
    match server.spawn {
        SpawnMode::Auto => {
            if is_git_repo_with_commit(&server.folder_path()) {
                SpawnMode::Worktree
            } else {
                SpawnMode::SameDir
            }
        }
        other => other,
    }
}

/// The arguments after `claude`, starting with `remote-control`.
pub fn remote_control_args(server: &Server, spawn: SpawnMode, resume: bool) -> Vec<String> {
    let mut args = vec![
        "remote-control".to_string(),
        "--name".to_string(),
        server.name.clone(),
    ];
    if spawn != SpawnMode::Auto {
        args.push("--spawn".to_string());
        args.push(spawn.as_str().to_string());
    }
    if let Some(n) = server.capacity {
        args.push("--capacity".to_string());
        args.push(n.to_string());
    }
    match server.create_session_in_dir {
        Some(true) => args.push("--create-session-in-dir".to_string()),
        Some(false) => args.push("--no-create-session-in-dir".to_string()),
        None => {}
    }
    if let Some(mode) = &server.permission_mode {
        args.push("--permission-mode".to_string());
        args.push(mode.clone());
    }
    match server.chrome {
        Some(true) => args.push("--chrome".to_string()),
        Some(false) => args.push("--no-chrome".to_string()),
        None => {}
    }
    if let Some(prefix) = &server.name_prefix {
        args.push("--remote-control-session-name-prefix".to_string());
        args.push(prefix.clone());
    }
    if resume {
        args.push("--continue".to_string());
    }
    args.extend(server.extra_args.iter().cloned());
    args
}

/// A copy-pasteable command line, for "Command that runs" and `hb status`.
pub fn display_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn shell_quote(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c));
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Server;

    #[test]
    fn script_equivalent_command() {
        let s = Server::new("myapp", "MyApp", "/tmp");
        let args = remote_control_args(&s, SpawnMode::Worktree, false);
        assert_eq!(
            args,
            [
                "remote-control",
                "--name",
                "MyApp",
                "--spawn",
                "worktree",
                "--no-create-session-in-dir"
            ]
        );
    }

    #[test]
    fn every_option_maps_to_a_flag() {
        let mut s = Server::new("x", "My App", "/tmp");
        s.capacity = Some(4);
        s.create_session_in_dir = Some(true);
        s.permission_mode = Some("plan".into());
        s.chrome = Some(false);
        s.name_prefix = Some("mac".into());
        s.extra_args = vec!["--verbose".into()];
        let args = remote_control_args(&s, SpawnMode::SameDir, true);
        assert_eq!(args[0], "remote-control", "no flags before the subcommand");
        for flag in [
            "--capacity",
            "--create-session-in-dir",
            "--permission-mode",
            "--no-chrome",
            "--remote-control-session-name-prefix",
            "--continue",
        ] {
            assert!(args.iter().any(|a| a == flag), "missing {flag}");
        }
        assert_eq!(args.last().unwrap(), "--verbose");
    }

    #[test]
    fn nulls_pass_nothing() {
        let mut s = Server::new("x", "X", "/tmp");
        s.create_session_in_dir = None;
        let args = remote_control_args(&s, SpawnMode::SameDir, false);
        assert_eq!(
            args,
            ["remote-control", "--name", "X", "--spawn", "same-dir"]
        );
    }

    #[test]
    fn quoting() {
        assert_eq!(
            display_command("/bin/claude", &["--name".into(), "Bill's App".into()]),
            r"/bin/claude --name 'Bill'\''s App'"
        );
    }

    #[test]
    fn auto_spawn_follows_git() {
        let dir = tempfile::tempdir().unwrap();
        let s = Server::new("x", "X", dir.path().to_str().unwrap());
        assert_eq!(resolve_spawn(&s), SpawnMode::SameDir);
    }
}
