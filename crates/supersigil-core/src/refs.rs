//! Criterion reference shape validation.
//!
//! A criterion reference has the form `document-id#criterion-id`. Both
//! the document and criterion fragments must be non-empty, and the
//! criterion fragment must not contain additional `#` characters.

/// Split a criterion reference string into its document and criterion parts.
///
/// Returns `None` if the string does not match the expected
/// `document-id#criterion-id` form.
#[must_use]
pub fn split_criterion_ref(s: &str) -> Option<(&str, &str)> {
    let (doc_id, target_id) = s.split_once('#')?;
    if doc_id.is_empty() || target_id.is_empty() || target_id.contains('#') {
        return None;
    }
    Some((doc_id, target_id))
}

/// Check whether a string is a valid criterion reference.
#[must_use]
pub fn is_valid_criterion_ref(s: &str) -> bool {
    split_criterion_ref(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_ref() {
        assert_eq!(
            split_criterion_ref("req/auth#crit-1"),
            Some(("req/auth", "crit-1"))
        );
    }

    #[test]
    fn missing_fragment() {
        assert_eq!(split_criterion_ref("req/auth"), None);
    }

    #[test]
    fn empty_fragment() {
        assert_eq!(split_criterion_ref("req/auth#"), None);
    }

    #[test]
    fn empty_document() {
        assert_eq!(split_criterion_ref("#crit-1"), None);
    }

    #[test]
    fn multi_hash_rejected() {
        assert_eq!(split_criterion_ref("doc#a#b"), None);
    }

    #[test]
    fn is_valid_delegates() {
        assert!(is_valid_criterion_ref("req/auth#crit-1"));
        assert!(!is_valid_criterion_ref("req/auth"));
        assert!(!is_valid_criterion_ref("doc#a#b"));
    }
}
