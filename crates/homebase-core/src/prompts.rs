//! Text that means a server is waiting on a person.
//!
//! `claude`'s wording changes between versions, so every pattern lives here
//! and nowhere else. Homebase only ever *detects* these; it never answers a
//! prompt on the user's behalf. Check these against each new claude release.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Attention {
    /// The folder-trust question. Fix: run `claude` once in the folder and accept.
    Trust,
    /// Signed out or the wrong kind of sign-in. Fix: `claude auth login`.
    SignIn,
}

impl Attention {
    pub fn message(self) -> &'static str {
        match self {
            Attention::Trust => {
                "waiting for the folder-trust question; run `claude` once in this folder, accept, then restart the server"
            }
            Attention::SignIn => {
                "claude isn't signed in with a subscription; run `claude auth login`, then restart the server"
            }
        }
    }
}

/// Lower-case substrings. Order matters: the first match wins.
pub const PATTERNS: &[(&str, Attention)] = &[
    ("do you trust the files in this folder", Attention::Trust),
    ("workspace not trusted", Attention::Trust),
    ("trust this folder", Attention::Trust),
    ("[y/n]", Attention::Trust),
    ("not logged in", Attention::SignIn),
    ("please run /login", Attention::SignIn),
    ("run `claude auth login`", Attention::SignIn),
    ("claude auth login", Attention::SignIn),
    ("oauth token has expired", Attention::SignIn),
    ("requires a claude.ai subscription", Attention::SignIn),
];

pub fn detect(text: &str) -> Option<Attention> {
    let lower = text.to_lowercase();
    PATTERNS
        .iter()
        .find(|(p, _)| lower.contains(p))
        .map(|(_, a)| *a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects() {
        assert_eq!(
            detect("Do you trust the files in this folder? [y/N] "),
            Some(Attention::Trust)
        );
        assert_eq!(
            detect("Error: Workspace not trusted"),
            Some(Attention::Trust)
        );
        assert_eq!(detect("You are not logged in."), Some(Attention::SignIn));
        assert_eq!(detect("Remote Control connected"), None);
    }
}
