//! The control socket protocol: one JSON object per line each way.
//! A client sends a [`Request`] and reads one [`Response`].
//!
//! The menu bar app speaks this too, so changes must stay backwards compatible:
//! add fields with defaults, never rename.

use crate::config::Server;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    Ping,
    /// Status of every configured server.
    List,
    Status {
        id: String,
    },
    /// No ids and `all: false` starts the servers marked `autoStart`.
    Start {
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    Stop {
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    /// Returns as soon as the agent has the request.
    Restart {
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    Add {
        server: Server,
    },
    /// Replaces the server with the same id. Restarts it if running and `restart` is true.
    Edit {
        server: Server,
        #[serde(default = "yes")]
        restart: bool,
    },
    /// Stops the server, then deletes it from the config. The folder is never touched.
    Remove {
        id: String,
    },
    GetConfig,
    /// Stops every server and exits the agent.
    Shutdown,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn ok(data: impl Serialize) -> Self {
        Response {
            ok: true,
            data: Some(serde_json::to_value(data).unwrap_or(serde_json::Value::Null)),
            error: None,
        }
    }

    pub fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        Response {
            ok: false,
            data: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    NotFound,
    Invalid,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerState {
    Stopped,
    Starting,
    Running,
    /// Waiting on a prompt, or claude is signed out.
    NeedsAttention,
    /// Exited and won't restart; `lastLines` keeps why.
    Failed,
    /// Waiting to restart.
    BackingOff,
}

impl ServerState {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerState::Stopped => "stopped",
            ServerState::Starting => "starting",
            ServerState::Running => "running",
            ServerState::NeedsAttention => "needs-attention",
            ServerState::Failed => "failed",
            ServerState::BackingOff => "backing-off",
        }
    }

    /// A process exists for the server.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            ServerState::Starting | ServerState::Running | ServerState::NeedsAttention
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub state: ServerState,
    pub pid: Option<u32>,
    /// Unix seconds.
    pub started_at: Option<u64>,
    pub restarts: u32,
    pub last_exit: Option<i32>,
    /// Why the server needs attention, when it does.
    pub attention: Option<String>,
    /// The command line it runs (or would run).
    pub command: Option<String>,
    /// The last lines of output, kept after it exits.
    #[serde(default)]
    pub last_lines: Vec<String>,
    pub log_file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pong {
    pub version: String,
    pub protocol: u32,
    pub pid: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let r = Request::Start {
            ids: vec!["a".into()],
            all: false,
        };
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"cmd":"start","ids":["a"],"all":false}"#
        );
        let parsed: Request = serde_json::from_str(r#"{"cmd":"stop","all":true}"#).unwrap();
        assert_eq!(
            parsed,
            Request::Stop {
                ids: vec![],
                all: true
            }
        );
        let parsed: Request = serde_json::from_str(r#"{"cmd":"get-config"}"#).unwrap();
        assert_eq!(parsed, Request::GetConfig);
    }

    #[test]
    fn error_response() {
        let r = Response::err(ErrorCode::NotFound, "no server 'x'");
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"ok":false,"error":{"code":"not-found","message":"no server 'x'"}}"#
        );
    }
}
