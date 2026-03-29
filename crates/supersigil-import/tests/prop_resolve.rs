mod generators;

use proptest::prelude::*;
use supersigil_import::ids::make_criterion_id;
use supersigil_import::parse::RawRef;
use supersigil_import::parse::requirements::{
    ParsedCriterion, ParsedRequirement, ParsedRequirements,
};
use supersigil_import::refs::{RequirementIndex, resolve_refs};
use supersigil_rust::verifies;

/// Build a `ParsedRequirements` that contains exactly the given refs as
/// requirement sections with matching criteria.
fn requirements_containing(refs: &[RawRef]) -> ParsedRequirements {
    // Group refs by requirement number to build one ParsedRequirement per unique number.
    let mut req_map: std::collections::BTreeMap<&str, Vec<&RawRef>> =
        std::collections::BTreeMap::new();
    for r in refs {
        req_map.entry(&r.requirement_number).or_default().push(r);
    }

    let requirements = req_map
        .into_iter()
        .map(|(num, group)| {
            let criteria = group
                .iter()
                .map(|r| ParsedCriterion {
                    index: r.criterion_index.clone(),
                    text: format!("THE System SHALL handle criterion {}", r.criterion_index),
                })
                .collect();
            ParsedRequirement {
                number: num.to_string(),
                title: Some(format!("Requirement {num}")),
                user_story: None,
                criteria,
                extra_prose: Vec::new(),
            }
        })
        .collect();

    ParsedRequirements {
        title: None,
        introduction: String::new(),
        glossary: None,
        requirements,
    }
}

// Feature: kiro-import, Property 7: Validates reference resolution
// Validates: Requirements 6.1, 6.2, 6.3, 7.4, 7.5
proptest! {
    /// All resolvable refs produce correct criterion ref strings.
    #[verifies("kiro-import/req#req-3-3")]
    #[test]
    fn prop_resolvable_refs_produce_correct_strings(
        refs in generators::arb_raw_ref_list(),
        feature_name in generators::arb_feature_name(),
    ) {
        let requirements = requirements_containing(&refs);
        let doc_id_base = format!("{feature_name}/req");

        let index = RequirementIndex::new(&requirements);
        let (resolved, markers) = resolve_refs(&refs, &index, &doc_id_base);

        // Every ref should resolve — no ambiguity markers.
        prop_assert!(markers.is_empty(),
            "Expected no ambiguity markers for resolvable refs, got: {markers:?}");

        // Should get one resolved string per input ref.
        prop_assert_eq!(resolved.len(), refs.len(),
            "Expected {} resolved refs, got {}", refs.len(), resolved.len());

        // Each resolved string should match the expected format.
        for (raw, resolved_str) in refs.iter().zip(resolved.iter()) {
            let crit_id = make_criterion_id(&raw.requirement_number, &raw.criterion_index);
            let expected = format!("{doc_id_base}#{crit_id}");
            prop_assert_eq!(resolved_str, &expected,
                "Resolved ref mismatch for {:?}", raw);
        }
    }

    /// Unresolvable refs (requirement number not in parsed requirements) emit ambiguity markers.
    #[test]
    fn prop_unresolvable_refs_emit_markers(
        refs in generators::arb_raw_ref_list(),
    ) {
        // Empty requirements — nothing can resolve.
        let empty_reqs = ParsedRequirements {
            title: None,
            introduction: String::new(),
            glossary: None,
            requirements: Vec::new(),
        };
        let doc_id_base = "test-feature/req";

        let index = RequirementIndex::new(&empty_reqs);
        let (resolved, markers) = resolve_refs(&refs, &index, doc_id_base);

        // No refs should resolve.
        prop_assert!(resolved.is_empty(),
            "Expected no resolved refs against empty requirements, got: {resolved:?}");

        // Each unresolvable ref should produce a marker.
        prop_assert_eq!(markers.len(), refs.len(),
            "Expected {} ambiguity markers, got {}", refs.len(), markers.len());

        for marker in &markers {
            prop_assert!(marker.contains("TODO(supersigil-import)"),
                "Marker should contain TODO prefix: {marker}");
        }
    }

    /// Mixed resolvable and unresolvable refs: resolvable ones resolve,
    /// unresolvable ones get markers.
    #[test]
    fn prop_mixed_refs_partial_resolution(
        resolvable in generators::arb_raw_ref_list(),
        unresolvable_idx in generators::arb_criterion_index(),
    ) {
        let requirements = requirements_containing(&resolvable);
        let doc_id_base = "mixed-feature/req";

        // Create an unresolvable ref with a requirement number that doesn't exist.
        let bad_ref = RawRef {
            requirement_number: "99999".to_string(),
            criterion_index: unresolvable_idx,
        };

        // Combine: resolvable refs first, then the bad one.
        let mut all_refs = resolvable.clone();
        all_refs.push(bad_ref.clone());

        let index = RequirementIndex::new(&requirements);
        let (resolved, markers) = resolve_refs(&all_refs, &index, doc_id_base);

        // The resolvable refs should all resolve.
        prop_assert_eq!(resolved.len(), resolvable.len(),
            "Expected {} resolved refs, got {}", resolvable.len(), resolved.len());

        // The unresolvable ref should produce exactly one marker.
        prop_assert_eq!(markers.len(), 1,
            "Expected 1 ambiguity marker for the bad ref, got {}", markers.len());

        prop_assert!(markers[0].contains("TODO(supersigil-import)"),
            "Marker should contain TODO prefix: {}", markers[0]);
    }

    /// Refs with matching requirement number but wrong criterion index emit markers.
    #[test]
    fn prop_wrong_criterion_index_emits_marker(
        req_num in generators::arb_requirement_number(),
        good_idx in generators::arb_criterion_index(),
    ) {
        // Build requirements with one criterion.
        let requirements = requirements_containing(&[RawRef {
            requirement_number: req_num.clone(),
            criterion_index: good_idx.clone(),
        }]);
        let doc_id_base = "idx-test/req";

        // Try to resolve a ref with the right requirement but a non-existent criterion.
        let bad_ref = RawRef {
            requirement_number: req_num,
            criterion_index: "zzz_nonexistent".to_string(),
        };

        let index = RequirementIndex::new(&requirements);
        let (resolved, markers) = resolve_refs(&[bad_ref], &index, doc_id_base);

        prop_assert!(resolved.is_empty(),
            "Should not resolve ref with wrong criterion index");
        prop_assert_eq!(markers.len(), 1,
            "Should emit one ambiguity marker for wrong criterion index");
    }
}
