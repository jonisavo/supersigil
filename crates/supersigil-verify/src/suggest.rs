//! Edit-distance suggestions for broken references.
//!
//! Provides a "did you mean?" facility that compares a broken reference
//! against known document IDs and returns the closest match above a
//! similarity threshold.

/// Minimum similarity score (0.0–1.0) for a candidate to be suggested.
const SIMILARITY_THRESHOLD: f64 = 0.6;

/// Find the closest match for `input` among `candidates` using normalized
/// Damerau-Levenshtein similarity.
///
/// Returns `None` when no candidate exceeds [`SIMILARITY_THRESHOLD`].
#[must_use]
pub fn closest_match<'a>(
    input: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut best: Option<(&str, f64)> = None;

    for candidate in candidates {
        let score = strsim::normalized_damerau_levenshtein(input, candidate);
        if score >= SIMILARITY_THRESHOLD && best.is_none_or(|(_, prev)| score > prev) {
            best = Some((candidate, score));
        }
    }

    best.map(|(candidate, _)| candidate)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_returns_candidate() {
        let candidates = ["auth/req", "design/auth", "tasks/auth"];
        assert_eq!(
            closest_match("auth/req", candidates.iter().copied()),
            Some("auth/req")
        );
    }

    #[test]
    fn typo_suggests_closest() {
        let candidates = ["auth/req", "design/auth", "tasks/auth"];
        // "auth/reqs" is one character off from "auth/req"
        assert_eq!(
            closest_match("auth/reqs", candidates.iter().copied()),
            Some("auth/req")
        );
    }

    #[test]
    fn transposition_suggests_closest() {
        let candidates = ["auth/req", "design/auth"];
        // "auth/erq" is a transposition of "auth/req"
        assert_eq!(
            closest_match("auth/erq", candidates.iter().copied()),
            Some("auth/req")
        );
    }

    #[test]
    fn completely_different_returns_none() {
        let candidates = ["auth/req", "design/auth"];
        assert_eq!(closest_match("zzzzzzz", candidates.iter().copied()), None);
    }

    #[test]
    fn empty_candidates_returns_none() {
        let candidates: Vec<&str> = vec![];
        assert_eq!(closest_match("auth/req", candidates.iter().copied()), None);
    }

    #[test]
    fn picks_best_among_multiple_similar() {
        let candidates = ["auth/requirements", "auth/reqs", "auth/req"];
        // "auth/req" should match "auth/req" exactly over "auth/reqs"
        assert_eq!(
            closest_match("auth/req", candidates.iter().copied()),
            Some("auth/req")
        );
    }

    #[test]
    fn short_ids_with_typo() {
        let candidates = ["a/b", "c/d", "e/f"];
        // "a/c" vs "a/b" — short strings, similarity may be below threshold
        let result = closest_match("a/c", candidates.iter().copied());
        // With 3-char strings, one char difference gives ~0.67 similarity
        // which is above 0.6, so it should suggest "a/b"
        assert_eq!(result, Some("a/b"));
    }
}
