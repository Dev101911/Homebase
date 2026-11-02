//! Shared pieces of Homebase: the server model and its config file, the
//! socket protocol between the agent (`hbd`) and its clients, finding the
//! user's own `claude`, and the helpers that read its terminal output.
//!
//! Homebase never bundles, modifies or signs in to Claude Code. It runs the
//! user's own `claude remote-control` and nothing else.

pub mod ansi;
pub mod claude;
pub mod client;
pub mod config;
pub mod paths;
pub mod prompts;
pub mod protocol;

pub use config::{Config, RestartPolicy, Server, SpawnMode};
pub use paths::Paths;
pub use protocol::{ErrorCode, Request, Response, ServerState, ServerStatus};
