//! Property-based tests for reference parsing and resolution.

mod generators;

use proptest::prelude::*;
use supersigil_import::parse::RawRef;
use supersigil_import::refs::parse_requirement_refs;

/// Format a list of `RawRef` pairs as a `Requirements X.Y, Z.W` string.
fn format_refs_as_requirements_string(refs: &[RawRef]) -> String {
    let parts: Vec<String> = refs.iter().map(std::string::ToString::to_string).collect();
    format!("Requirements {}", parts.join(", "))
}

/// Format a list of `RawRef` pairs as a bare `X.Y, Z.W` string (no prefix).
fn format_refs_bare(refs: &[RawRef]) -> String {
    refs.iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

// Feature: kiro-import, Property 4: Requirement ref parsing round-trip
// Validates: Requirements 20.1, 20.2, 20.3, 20.4
proptest! {
    /// Single ref with `Requirements` prefix round-trips.
    #[test]
    fn prop_single_ref_round_trip(
        raw_ref in generators::arb_raw_ref(),
    ) {
        let input = format!(
            "Requirements {}.{}",
            raw_ref.requirement_number, raw_ref.criterion_index
        );
        let (parsed, markers) = parse_requirement_refs(&input);

        prop_assert_eq!(parsed.len(), 1,
            "Expected 1 ref from {:?}, got {:?}", input, parsed);
        prop_assert_eq!(&parsed[0], &raw_ref,
            "Parsed ref mismatch for {:?}", input);
        prop_assert!(markers.is_empty(),
            "Unexpected ambiguity markers for {:?}: {:?}", input, markers);
    }

    /// Comma-separated ref list with `Requirements` prefix round-trips.
    #[test]
    fn prop_ref_list_with_prefix_round_trip(
        refs in generators::arb_raw_ref_list(),
    ) {
        let input = format_refs_as_requirements_string(&refs);
        let (parsed, markers) = parse_requirement_refs(&input);

        prop_assert_eq!(parsed.len(), refs.len(),
            "Count mismatch for {:?}: expected {}, got {}",
            input, refs.len(), parsed.len());

        for (expected, actual) in refs.iter().zip(parsed.iter()) {
            prop_assert_eq!(actual, expected,
                "Ref mismatch in {:?}", input);
        }

        prop_assert!(markers.is_empty(),
            "Unexpected ambiguity markers for {:?}: {:?}", input, markers);
    }

    /// Comma-separated ref list without `Requirements` prefix round-trips.
    #[test]
    fn prop_ref_list_bare_round_trip(
        refs in generators::arb_raw_ref_list(),
    ) {
        let input = format_refs_bare(&refs);
        let (parsed, markers) = parse_requirement_refs(&input);

        prop_assert_eq!(parsed.len(), refs.len(),
            "Count mismatch for bare {:?}: expected {}, got {}",
            input, refs.len(), parsed.len());

        for (expected, actual) in refs.iter().zip(parsed.iter()) {
            prop_assert_eq!(actual, expected,
                "Ref mismatch in bare {:?}", input);
        }

        prop_assert!(markers.is_empty(),
            "Unexpected ambiguity markers for bare {:?}: {:?}", input, markers);
    }

    /// Multiple criteria from the same requirement round-trip.
    #[test]
    fn prop_same_requirement_multiple_criteria(
        req_num in generators::arb_requirement_number(),
        indices in proptest::collection::vec(generators::arb_criterion_index(), 2..5),
    ) {
        let refs: Vec<RawRef> = indices
            .iter()
            .map(|idx| RawRef {
                requirement_number: req_num.clone(),
                criterion_index: idx.clone(),
            })
            .collect();

        let input = format_refs_as_requirements_string(&refs);
        let (parsed, markers) = parse_requirement_refs(&input);

        prop_assert_eq!(parsed.len(), refs.len(),
            "Count mismatch for same-req {:?}", input);

        for (expected, actual) in refs.iter().zip(parsed.iter()) {
            prop_assert_eq!(actual, expected);
        }

        prop_assert!(markers.is_empty(),
            "Unexpected markers for same-req {:?}: {:?}", input, markers);
    }
}

// Feature: kiro-import, Property 5: Requirement ref range expansion
// Validates: Requirements 20.5
proptest! {
    /// Numeric range `X.Y–X.Z` (en-dash) or `X.Y-X.Z` (hyphen) expands to individual refs.
    #[test]
    fn prop_numeric_range_expansion(
        req_num in generators::arb_requirement_number(),
        start in 1..10u32,
        span in 1..6u32,
        separator in prop_oneof![Just("\u{2013}"), Just("-")],
    ) {
        let end = start + span;
        let input = format!("{req_num}.{start}{separator}{req_num}.{end}");
        let (parsed, markers) = parse_requirement_refs(&input);

        let expected_count = (end - start + 1) as usize;
        prop_assert_eq!(parsed.len(), expected_count);

        for (i, r) in parsed.iter().enumerate() {
            let expected_idx = (start as usize) + i;
            prop_assert_eq!(&r.requirement_number, &req_num);
            prop_assert_eq!(&r.criterion_index, &expected_idx.to_string());
        }

        prop_assert!(markers.is_empty(),
            "Numeric range should not produce markers: {:?}", markers);
    }

    /// Non-numeric range indices emit ambiguity markers.
    #[test]
    fn prop_non_numeric_range_emits_marker(
        req_num in generators::arb_requirement_number(),
        suffix_a in prop::sample::select(vec!['a', 'b', 'c']),
        suffix_b in prop::sample::select(vec!['d', 'e', 'f']),
        start in 1..10u32,
        end in 1..10u32,
    ) {
        let input = format!(
            "{req_num}.{start}{suffix_a}\u{2013}{req_num}.{end}{suffix_b}"
        );
        let (_parsed, markers) = parse_requirement_refs(&input);

        prop_assert!(!markers.is_empty(),
            "Non-numeric range {:?} should produce ambiguity markers", input);
    }
}

// Feature: kiro-import, Property 6: Unparseable ref detection
// Validates: Requirements 20.6
proptest! {
    /// Bare numbers without dots produce ambiguity markers.
    #[test]
    fn prop_bare_number_emits_marker(
        num in generators::arb_requirement_number(),
    ) {
        // A bare number like "5" doesn't match X.Y pattern
        let input = format!("Requirements {num}");
        let (parsed, markers) = parse_requirement_refs(&input);

        prop_assert!(parsed.is_empty(),
            "Bare number {:?} should not produce refs, got {:?}", input, parsed);
        prop_assert!(!markers.is_empty(),
            "Bare number {:?} should produce ambiguity markers", input);
    }

    /// Tokens mixed with valid refs: valid refs parsed, invalid tokens get markers.
    #[test]
    fn prop_mixed_valid_and_invalid_tokens(
        valid_ref in generators::arb_raw_ref(),
        bare_num in generators::arb_requirement_number(),
    ) {
        let input = format!(
            "{}.{}, {bare_num}",
            valid_ref.requirement_number, valid_ref.criterion_index
        );
        let (parsed, markers) = parse_requirement_refs(&input);

        // The valid ref should be parsed
        prop_assert!(parsed.contains(&valid_ref),
            "Valid ref {:?} should be in parsed results {:?}", valid_ref, parsed);

        // The bare number should produce a marker
        prop_assert!(!markers.is_empty(),
            "Bare number token should produce ambiguity marker for {:?}", input);
    }
}
