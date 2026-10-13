// //// Neoffice — added file (no upstream equivalent): errors of the Nora client core.

use std::fmt;

/// What can go wrong between an editor and Nora. Nothing here panics: every failure is a value
/// the panel can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoraError {
    /// The server answered with a non-success HTTP status and no message of its own.
    Http(u16),
    /// The server (Frappe) refused the call and said why.
    Server(String),
    /// The answer was not what the protocol promises.
    Malformed(String),
}

impl NoraError {
    /// Whether the user probably has to sign in again (the host decides what to do about it).
    pub fn is_auth(&self) -> bool {
        matches!(self, NoraError::Http(401 | 403))
    }
}

impl fmt::Display for NoraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NoraError::Http(status) => write!(f, "the server answered HTTP {status}"),
            NoraError::Server(message) => write!(f, "{message}"),
            NoraError::Malformed(what) => write!(f, "unexpected answer from Nora: {what}"),
        }
    }
}

impl std::error::Error for NoraError {}
