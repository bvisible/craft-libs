// //// Neoffice — added file (no upstream equivalent): errors of the Nora client core.

use std::fmt;

/// What can go wrong between an editor and Nora. Nothing here panics: every failure is a value
/// the panel can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoraError {
    /// The server answered with a non-success HTTP status and no message of its own.
    Http(u16),
    /// The server (Frappe) refused the call with this status and said why.
    Server { status: u16, message: String },
    /// No answer came at all (the host reports it as status 0): the server could not be reached.
    /// Holds the host's reason.
    Unreachable(String),
    /// The answer was not what the protocol promises.
    Malformed(String),
    /// Nora said nothing for longer than the session waits.
    Timeout,
}

impl NoraError {
    /// Whether the user probably has to sign in again (the host decides what to do about it). Frappe
    /// answers 403 with a message to a call whose session has expired.
    pub fn is_auth(&self) -> bool {
        matches!(self, NoraError::Http(401 | 403) | NoraError::Server { status: 401 | 403, .. })
    }

    /// The HTTP status of the failure, when the server answered.
    pub fn status(&self) -> Option<u16> {
        match self {
            NoraError::Http(status) | NoraError::Server { status, .. } => Some(*status),
            NoraError::Unreachable(_) | NoraError::Malformed(_) | NoraError::Timeout => None,
        }
    }
}

/// Nora's failures in the shared vocabulary of `craft-ai-core`, for a host that handles them as it
/// handles those of the other AI connections (sign in again, try again later, or not).
#[cfg(feature = "ai-core")]
impl NoraError {
    pub fn kind(&self) -> craft_ai_core::ErrorKind {
        use craft_ai_core::ErrorKind;
        if self.is_auth() {
            return ErrorKind::Auth;
        }
        match (self, self.status()) {
            (NoraError::Unreachable(_), _) | (_, Some(408 | 502 | 503 | 504)) => ErrorKind::Network,
            (_, Some(429)) => ErrorKind::RateLimited,
            // Frappe's refusals of what was sent: a validation (417), a missing record (404)…
            (_, Some(400..500)) => ErrorKind::InvalidRequest,
            _ => ErrorKind::Provider,
        }
    }

    /// The same failure as a `craft-ai-core` error, with this error's sentence as its message.
    pub fn to_ai_error(&self) -> craft_ai_core::AiError {
        craft_ai_core::AiError::new(self.kind(), self.to_string())
    }
}

impl fmt::Display for NoraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NoraError::Http(status) => write!(f, "the server answered HTTP {status}"),
            NoraError::Server { message, .. } | NoraError::Unreachable(message) => write!(f, "{message}"),
            NoraError::Malformed(what) => write!(f, "unexpected answer from Nora: {what}"),
            NoraError::Timeout => write!(f, "Nora did not answer in time"),
        }
    }
}

impl std::error::Error for NoraError {}
