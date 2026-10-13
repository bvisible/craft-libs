// //// Neoffice — added file (no upstream equivalent): what Nora may do in an open document.

/// What to do with a command Nora asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Decision {
    /// Run it.
    Allow,
    /// Run it only after the user clicked yes in the panel.
    Confirm,
    /// Never run it; Nora is told it was refused.
    Deny,
}

/// The commands Nora may play in an editor, by command-id prefix (`file.`, `format.`…).
///
/// An editor's commands are its own (`engine.execute {command, params}`), so the policy is a list
/// of prefixes the app tunes. The default refuses everything that opens, saves, closes or quits a
/// document (`file.`) or the application (`app.`): Nora edits the document that is open, and saving
/// is a separate action that always asks the user first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPolicy {
    deny: Vec<String>,
    confirm: Vec<String>,
}

impl Default for CommandPolicy {
    fn default() -> Self {
        CommandPolicy { deny: vec!["file.".into(), "app.".into()], confirm: Vec::new() }
    }
}

impl CommandPolicy {
    /// A policy that refuses nothing. Only for tests and for apps that filter elsewhere.
    pub fn permissive() -> Self {
        CommandPolicy { deny: Vec::new(), confirm: Vec::new() }
    }

    /// Also refuse every command starting with `prefix`.
    pub fn deny_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.deny.push(prefix.into());
        self
    }

    /// Ask the user before running any command starting with `prefix`.
    pub fn confirm_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.confirm.push(prefix.into());
        self
    }

    /// The decision for one command id. Refusal wins over confirmation.
    pub fn check(&self, command: &str) -> Decision {
        if self.deny.iter().any(|prefix| command.starts_with(prefix.as_str())) {
            Decision::Deny
        } else if self.confirm.iter().any(|prefix| command.starts_with(prefix.as_str())) {
            Decision::Confirm
        } else {
            Decision::Allow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_refuses_files_and_the_application() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.check("file.saveAs"), Decision::Deny);
        assert_eq!(policy.check("app.quit"), Decision::Deny);
        assert_eq!(policy.check("format.bold"), Decision::Allow);
    }

    #[test]
    fn confirmation_and_refusal_stack_with_refusal_first() {
        let policy = CommandPolicy::default().confirm_prefix("edit.delete").deny_prefix("edit.deleteAll");
        assert_eq!(policy.check("edit.deleteWord"), Decision::Confirm);
        assert_eq!(policy.check("edit.deleteAll"), Decision::Deny);
    }

    #[test]
    fn permissive_allows_everything_and_decisions_order_by_severity() {
        assert_eq!(CommandPolicy::permissive().check("file.close"), Decision::Allow);
        assert!(Decision::Allow < Decision::Confirm && Decision::Confirm < Decision::Deny);
    }
}
