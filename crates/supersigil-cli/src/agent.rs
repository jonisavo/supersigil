//! Whether a command runs inside an agent session.
//!
//! An agent may read reviews and judgments but never closes an item or a
//! review: judging is for people. The check reads markers the agent's
//! harness sets in the environment of the commands it runs, so it guards
//! against accidents, not malice; anyone can unset them.

use std::ffi::OsString;

/// Environment variables whose presence marks an agent session: Claude
/// Code sets both. Another agent's markers can join the list.
pub const AGENT_MARKERS: [&str; 2] = ["CLAUDECODE", "CLAUDE_CODE_SESSION_ID"];

/// Whether any of [`AGENT_MARKERS`] is set and non-empty, as `lookup` reads
/// the environment (`std::env::var_os` for the process's own).
#[must_use]
pub fn in_agent_session(lookup: impl Fn(&str) -> Option<OsString>) -> bool {
    AGENT_MARKERS
        .iter()
        .any(|marker| lookup(marker).is_some_and(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{AGENT_MARKERS, in_agent_session};

    /// An environment holding exactly `vars`.
    fn environment(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, OsString)> = vars
            .iter()
            .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
            .collect();
        move |name| {
            vars.iter()
                .find(|(var, _)| var == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn any_marker_set_and_non_empty_is_an_agent_session() {
        assert_eq!(AGENT_MARKERS, ["CLAUDECODE", "CLAUDE_CODE_SESSION_ID"]);
        assert!(in_agent_session(environment(&[("CLAUDECODE", "1")])));
        assert!(in_agent_session(environment(&[(
            "CLAUDE_CODE_SESSION_ID",
            "0b5e"
        )])));
        assert!(in_agent_session(environment(&[
            ("CLAUDECODE", ""),
            ("CLAUDE_CODE_SESSION_ID", "0b5e"),
        ])));
    }

    #[test]
    fn unset_or_empty_markers_are_no_agent_session() {
        assert!(!in_agent_session(environment(&[])));
        assert!(!in_agent_session(environment(&[
            ("CLAUDECODE", ""),
            ("CLAUDE_CODE_SESSION_ID", ""),
        ])));
        assert!(!in_agent_session(environment(&[("CLAUDE", "1")])));
    }
}
