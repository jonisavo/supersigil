//! Recomputes a file's content after an editing tool ran, from the retained
//! original and the tool's input. Returns `None` whenever the result cannot
//! be established, which the caller records as an unknown state.

/// Applies one `Edit` replacement. `None` if `old` is empty or absent.
#[must_use]
pub fn apply_edit(original: &str, old: &str, new: &str, replace_all: bool) -> Option<String> {
    if old.is_empty() || !original.contains(old) {
        return None;
    }
    Some(if replace_all {
        original.replace(old, new)
    } else {
        original.replacen(old, new, 1)
    })
}

/// Applies `MultiEdit` replacements in order. `None` if any step fails.
#[must_use]
pub fn apply_multi_edit(original: &str, edits: &[(String, String, bool)]) -> Option<String> {
    edits
        .iter()
        .try_fold(original.to_owned(), |current, (old, new, all)| {
            apply_edit(&current, old, new, *all)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_occurrence_or_all() {
        assert_eq!(
            apply_edit("a b a", "a", "c", false).as_deref(),
            Some("c b a")
        );
        assert_eq!(
            apply_edit("a b a", "a", "c", true).as_deref(),
            Some("c b c")
        );
    }

    #[test]
    fn missing_or_empty_old_is_unknown() {
        assert_eq!(apply_edit("a", "z", "c", false), None);
        assert_eq!(apply_edit("a", "", "c", false), None);
    }

    #[test]
    fn multi_edit_is_sequential() {
        let edits = vec![
            ("a".to_owned(), "b".to_owned(), false),
            ("b".to_owned(), "c".to_owned(), true),
        ];
        assert_eq!(apply_multi_edit("a b", &edits).as_deref(), Some("c c"));
        let bad = vec![("q".to_owned(), "r".to_owned(), false)];
        assert_eq!(apply_multi_edit("a", &bad), None);
    }
}
