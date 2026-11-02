//! `servers.json`: the servers people configure, readable and editable by hand.
//! Runtime state never goes in this file.

use crate::paths::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub version: u32,
    /// `null` means: look for `claude` on the login shell's PATH, then in the usual places.
    #[serde(default)]
    pub claude_path: Option<String>,
    #[serde(default)]
    pub servers: Vec<Server>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: CONFIG_VERSION,
            claude_path: None,
            servers: Vec::new(),
        }
    }
}

/// One `claude remote-control` process for one folder.
///
/// Every `Option` maps to one flag; `None` (`null` in JSON) means the flag is
/// not passed, so `claude`'s own default applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// Lower-case slug, unique, fixed once created.
    pub id: String,
    /// What the Claude app shows (`--name`).
    pub name: String,
    /// The working directory. `~` is expanded.
    pub folder: String,
    #[serde(default)]
    pub spawn: SpawnMode,
    /// `--create-session-in-dir` / `--no-create-session-in-dir`.
    #[serde(default)]
    pub create_session_in_dir: Option<bool>,
    /// `--capacity`.
    #[serde(default)]
    pub capacity: Option<u32>,
    /// `--permission-mode`.
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// `--chrome` / `--no-chrome`.
    #[serde(default)]
    pub chrome: Option<bool>,
    /// `--remote-control-session-name-prefix`.
    #[serde(default)]
    pub name_prefix: Option<String>,
    /// Passed after the flags above.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Added to the environment `claude` runs with.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub auto_start: bool,
    #[serde(default)]
    pub restart: RestartPolicy,
}

fn default_true() -> bool {
    true
}

impl Server {
    /// A server with the defaults `hb add` uses.
    pub fn new(id: impl Into<String>, name: impl Into<String>, folder: impl Into<String>) -> Self {
        Server {
            id: id.into(),
            name: name.into(),
            folder: folder.into(),
            spawn: SpawnMode::Auto,
            create_session_in_dir: Some(false),
            capacity: None,
            permission_mode: None,
            chrome: None,
            name_prefix: None,
            extra_args: Vec::new(),
            env: BTreeMap::new(),
            auto_start: true,
            restart: RestartPolicy::OnFailure,
        }
    }

    pub fn folder_path(&self) -> PathBuf {
        expand_home(&self.folder)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpawnMode {
    /// `worktree` when the folder is a git repo with at least one commit, else `same-dir`.
    #[default]
    Auto,
    SameDir,
    Worktree,
    Session,
}

impl SpawnMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SpawnMode::Auto => "auto",
            SpawnMode::SameDir => "same-dir",
            SpawnMode::Worktree => "worktree",
            SpawnMode::Session => "session",
        }
    }
}

impl fmt::Display for SpawnMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SpawnMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(SpawnMode::Auto),
            "same-dir" => Ok(SpawnMode::SameDir),
            "worktree" => Ok(SpawnMode::Worktree),
            "session" => Ok(SpawnMode::Session),
            _ => Err(format!(
                "unknown spawn mode '{s}' (expected auto, same-dir, worktree or session)"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Never,
    #[default]
    OnFailure,
    Always,
}

impl RestartPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            RestartPolicy::Never => "never",
            RestartPolicy::OnFailure => "on-failure",
            RestartPolicy::Always => "always",
        }
    }
}

impl FromStr for RestartPolicy {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "never" => Ok(RestartPolicy::Never),
            "on-failure" => Ok(RestartPolicy::OnFailure),
            "always" => Ok(RestartPolicy::Always),
            _ => Err(format!(
                "unknown restart policy '{s}' (expected never, on-failure or always)"
            )),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_json::Error),
    Invalid(Vec<String>),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "can't read the config: {e}"),
            ConfigError::Parse(e) => write!(f, "the config isn't valid JSON: {e}"),
            ConfigError::Invalid(problems) => {
                write!(f, "the config is invalid: {}", problems.join("; "))
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Reads and validates the file. A missing file is an empty config.
    pub fn load(path: &Path) -> Result<Config, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(ConfigError::Io(e)),
        };
        let config: Config = serde_json::from_str(&text).map_err(ConfigError::Parse)?;
        config.validate().map_err(ConfigError::Invalid)?;
        Ok(config)
    }

    /// Writes the file atomically, pretty-printed.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        text.push('\n');
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if self.version != CONFIG_VERSION {
            problems.push(format!(
                "version is {}, but this Homebase reads version {CONFIG_VERSION}",
                self.version
            ));
        }
        let mut ids = HashSet::new();
        for s in &self.servers {
            if !is_valid_id(&s.id) {
                problems.push(format!(
                    "id '{}' must be lower-case letters, digits and dashes, starting with a letter or digit",
                    s.id
                ));
            }
            if !ids.insert(s.id.as_str()) {
                problems.push(format!("id '{}' is used twice", s.id));
            }
            if s.name.trim().is_empty() {
                problems.push(format!("server '{}' has an empty name", s.id));
            }
            if s.folder.trim().is_empty() {
                problems.push(format!("server '{}' has an empty folder", s.id));
            }
            if s.capacity == Some(0) {
                problems.push(format!("server '{}' has capacity 0", s.id));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    pub fn server(&self, id: &str) -> Option<&Server> {
        self.servers.iter().find(|s| s.id == id)
    }

    pub fn server_mut(&mut self, id: &str) -> Option<&mut Server> {
        self.servers.iter_mut().find(|s| s.id == id)
    }
}

pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// "My App" -> "my-app". Never empty.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-');
    let mut slug: String = trimmed.chars().take(64).collect();
    slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() {
        "server".to_string()
    } else {
        slug
    }
}

/// `~/x` -> `$HOME/x`.
pub fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        home_dir()
    } else if let Some(rest) = path.strip_prefix("~/") {
        home_dir().join(rest)
    } else {
        PathBuf::from(path)
    }
}

/// `$HOME/x` -> `~/x`, so the config stays portable in a dotfiles repo.
pub fn contract_home(path: &Path) -> String {
    let home = home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if home != Path::new("/") => {
            if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            }
        }
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_EXAMPLE: &str = r#"{
      "version": 1,
      "claudePath": null,
      "servers": [
        {
          "id": "notes-app",
          "name": "NotesApp",
          "folder": "~/Projects/NotesApp",
          "spawn": "auto",
          "createSessionInDir": false,
          "capacity": null,
          "permissionMode": null,
          "chrome": null,
          "extraArgs": [],
          "env": {},
          "autoStart": true,
          "restart": "on-failure"
        }
      ]
    }"#;

    #[test]
    fn reads_the_design_doc_example() {
        let c: Config = serde_json::from_str(DOC_EXAMPLE).unwrap();
        c.validate().unwrap();
        let s = &c.servers[0];
        assert_eq!(s.id, "notes-app");
        assert_eq!(s.spawn, SpawnMode::Auto);
        assert_eq!(s.create_session_in_dir, Some(false));
        assert_eq!(s.restart, RestartPolicy::OnFailure);
        assert_eq!(s.name_prefix, None);
    }

    #[test]
    fn minimal_server_gets_defaults() {
        let c: Config = serde_json::from_str(
            r#"{"version":1,"servers":[{"id":"a","name":"A","folder":"/tmp"}]}"#,
        )
        .unwrap();
        let s = &c.servers[0];
        assert!(s.auto_start);
        assert_eq!(s.create_session_in_dir, None);
        assert_eq!(s.restart, RestartPolicy::OnFailure);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/servers.json");
        let mut c = Config::default();
        c.servers
            .push(Server::new("web", "Web", "~/Projects/web-site"));
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), c);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"capacity\": null"),
            "nulls are written out: {text}"
        );
    }

    #[test]
    fn missing_file_is_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            Config::load(&dir.path().join("nope.json")).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn validation_catches_problems() {
        let mut c = Config::default();
        c.servers.push(Server::new("a", "A", "/x"));
        c.servers.push(Server::new("a", "", "/y"));
        c.servers.push(Server::new("Bad Id", "B", ""));
        let problems = c.validate().unwrap_err();
        assert_eq!(problems.len(), 4, "{problems:?}");
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("My App"), "my-app");
        assert_eq!(slugify("  NotesApp!! "), "notesapp");
        assert_eq!(slugify("my_game"), "my-game");
        assert_eq!(slugify("???"), "server");
        assert!(is_valid_id(&slugify("Ünïcode Näme 2")));
    }

    #[test]
    fn home_paths() {
        let home = home_dir();
        assert_eq!(expand_home("~/a/b"), home.join("a/b"));
        assert_eq!(expand_home("/abs"), PathBuf::from("/abs"));
        assert_eq!(contract_home(&home.join("a/b")), "~/a/b");
    }
}
