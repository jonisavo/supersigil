/// Parsing of Kiro design documents.
pub mod design;
/// Parsing of Kiro requirements documents.
pub mod requirements;
/// Parsing of Kiro task documents.
pub mod tasks;

/// A raw requirement reference parsed from Kiro format (e.g., `X.Y`).
#[derive(Debug, Clone, PartialEq)]
pub struct RawRef {
    /// The requirement number (e.g., `1` in `1.3`).
    pub requirement_number: String,
    /// The criterion index within the requirement (e.g., `3` in `1.3`).
    pub criterion_index: String,
}

impl std::fmt::Display for RawRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.requirement_number, self.criterion_index)
    }
}

/// Join lines, trimming leading/trailing blank lines.
pub(crate) fn join_trimmed(lines: &[String]) -> String {
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].join("\n"),
        _ => String::new(),
    }
}
