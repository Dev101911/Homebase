//! Where Homebase keeps its files.
//!
//! Each location can be overridden with an environment variable, which tests
//! use to run an isolated agent:
//! `HOMEBASE_CONFIG_DIR`, `HOMEBASE_DATA_DIR` and `HOMEBASE_LOG_DIR`.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Holds `servers.json` and `settings.json`.
    pub config_dir: PathBuf,
    /// Holds the control socket and `state.json`.
    pub data_dir: PathBuf,
    /// Holds one log file per server, plus `hbd.log`.
    pub log_dir: PathBuf,
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

impl Paths {
    /// The standard locations, after environment overrides.
    pub fn from_env() -> Self {
        let home = home_dir();
        let config_dir =
            env_dir("HOMEBASE_CONFIG_DIR").unwrap_or_else(|| home.join(".config/homebase"));
        let (default_data, default_logs) = if cfg!(target_os = "macos") {
            (
                home.join("Library/Application Support/Homebase"),
                home.join("Library/Logs/Homebase"),
            )
        } else {
            (
                home.join(".local/share/homebase"),
                home.join(".local/state/homebase/logs"),
            )
        };
        Paths {
            config_dir,
            data_dir: env_dir("HOMEBASE_DATA_DIR").unwrap_or(default_data),
            log_dir: env_dir("HOMEBASE_LOG_DIR").unwrap_or(default_logs),
        }
    }

    pub fn servers_file(&self) -> PathBuf {
        self.config_dir.join("servers.json")
    }

    pub fn socket(&self) -> PathBuf {
        self.data_dir.join("hbd.sock")
    }

    pub fn server_log(&self, id: &str) -> PathBuf {
        self.log_dir.join(format!("{id}.log"))
    }

    pub fn agent_log(&self) -> PathBuf {
        self.log_dir.join("hbd.log")
    }
}
