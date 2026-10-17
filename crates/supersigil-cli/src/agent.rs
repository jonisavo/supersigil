//! Detects commands running inside an agent session.
//!
//! Agents may read reviews and judgments, but only people may close items or
//! reviews. This check reads environment markers set by the agent's harness.
//! It prevents accidents; anyone can unset the markers.

use std::ffi::OsString;

/// Environment markers for agent sessions. Claude Code sets both.
/// The list can include other agents' markers.
pub const AGENT_MARKERS: [&str; 2] = ["CLAUDECODE", "CLAUDE_CODE_SESSION_ID"];

/// Returns whether `lookup` finds a non-empty value for any of [`AGENT_MARKERS`].
/// Use `std::env::var_os` to read the process environment.
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
