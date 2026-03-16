use supersigil_core::{
    ALTERNATIVE, ComponentDefs, Config, DECISION, DocumentGraph, EXAMPLE, EXPECTED, RATIONALE,
    SpecDocument, VERIFIED_BY,
};

use crate::report::{Finding, RuleName};
use crate::rules::{find_components, has_component};
use crate::scan::TagMatch;

// ---------------------------------------------------------------------------
// check_required_components
// ---------------------------------------------------------------------------

/// For typed documents, check that all `required_components` from the config
/// type definition are present.
pub fn check_required_components(graph: &DocumentGraph, config: &Config) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (doc_id, doc) in graph.documents() {
        let Some(ref doc_type) = doc.frontmatter.doc_type else {
            continue;
        };
        let Some(type_def) = config.documents.types.get(doc_type) else {
            continue;
        };
        for required in &type_def.required_components {
            let has_it = has_component(&doc.components, required);
            if !has_it {
                findings.push(Finding::new(
                    RuleName::MissingRequiredComponent,
                    Some(doc_id.to_owned()),
                    format!(
                        "document `{doc_id}` (type `{doc_type}`) is missing required component `{required}`"
                    ),
                    None,
                ));
            }
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_id_pattern
// ---------------------------------------------------------------------------

/// If `config.id_pattern` is set, check that each document ID matches the regex.
pub fn check_id_pattern(graph: &DocumentGraph, config: &Config) -> Vec<Finding> {
    let Some(ref pattern) = config.id_pattern else {
        return Vec::new();
    };
    let Ok(re) = regex::Regex::new(pattern) else {
        return Vec::new(); // Invalid pattern is not this rule's problem
    };
    let mut findings = Vec::new();
    for (doc_id, _doc) in graph.documents() {
        if !re.is_match(doc_id) {
            findings.push(Finding::new(
                RuleName::InvalidIdPattern,
                Some(doc_id.to_owned()),
                format!("document ID `{doc_id}` does not match pattern `{pattern}`"),
                None,
            ));
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_isolated
// ---------------------------------------------------------------------------

/// Check each document for incoming or outgoing refs. Documents with neither
/// are flagged as isolated.
pub fn check_isolated(graph: &DocumentGraph) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (doc_id, doc) in graph.documents() {
        // Check outgoing refs (document has ref components)
        let has_outgoing = ["References", "Implements", "DependsOn"]
            .iter()
            .any(|name| has_component(&doc.components, name));

        // Check incoming refs (other docs reference this one)
        let has_incoming = !graph.references(doc_id, None).is_empty()
            || !graph.implements(doc_id).is_empty()
            || !graph.depends_on(doc_id).is_empty();

        // Check task-level implements (outgoing: this doc's tasks implement
        // criteria in another document)
        let has_task_level_refs = graph.task_order(doc_id).is_some_and(|order| {
            order.iter().any(|task_id| {
                graph
                    .task_implements(doc_id, task_id)
                    .is_some_and(|impls| impls.iter().any(|(target_doc, _)| target_doc != doc_id))
            })
        });

        // Check incoming task-level implements (other docs' tasks implement
        // criteria in this document)
        let has_incoming_task_refs = graph.documents().any(|(other_id, _)| {
            other_id != doc_id
                && graph.task_order(other_id).is_some_and(|order| {
                    order.iter().any(|task_id| {
                        graph
                            .task_implements(other_id, task_id)
                            .is_some_and(|impls| {
                                impls.iter().any(|(target_doc, _)| target_doc == doc_id)
                            })
                    })
                })
        });

        if !has_outgoing && !has_incoming && !has_task_level_refs && !has_incoming_task_refs {
            findings.push(Finding::new(
                RuleName::IsolatedDocument,
                Some(doc_id.to_owned()),
                format!("document `{doc_id}` has no incoming or outgoing references"),
                None,
            ));
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_orphan_tags
// ---------------------------------------------------------------------------

/// Check pre-scanned tag matches for tags not declared in any `VerifiedBy` component.
///
/// `tag_matches` should be pre-computed via [`crate::scan::scan_all_tags`].
pub fn check_orphan_tags(docs: &[&SpecDocument], tag_matches: &[TagMatch]) -> Vec<Finding> {
    // Collect declared tags from VerifiedBy components
    let mut declared_tags: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for doc in docs {
        for vb in find_components(&doc.components, VERIFIED_BY) {
            if vb.attributes.get("strategy").map(String::as_str) == Some("tag")
                && let Some(tag) = vb.attributes.get("tag")
            {
                declared_tags.insert(tag.as_str());
            }
        }
    }

    let mut findings = Vec::new();
    let mut seen_orphans: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for m in tag_matches {
        if !declared_tags.contains(m.tag.as_str()) && seen_orphans.insert(m.tag.as_str()) {
            findings.push(Finding::new(
                RuleName::OrphanTestTag,
                None,
                format!(
                    "tag `{}` found in test files but not declared in any VerifiedBy",
                    m.tag
                ),
                None,
            ));
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_verified_by_placement
// ---------------------------------------------------------------------------

/// Check that every `VerifiedBy` component is a direct child of a verifiable
/// component (e.g. `Criterion`). `VerifiedBy` at document root or under a
/// non-verifiable component is a structural error.
pub fn check_verified_by_placement(
    docs: &[&SpecDocument],
    component_defs: &ComponentDefs,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        walk_for_verified_by(doc_id, &doc.components, None, component_defs, &mut findings);
    }
    findings
}

/// Recursively walk the component tree. `parent_name` is the name of the
/// immediate parent component (or `None` at the document root level).
fn walk_for_verified_by(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    parent_name: Option<&str>,
    component_defs: &ComponentDefs,
    findings: &mut Vec<Finding>,
) {
    for comp in components {
        if comp.name == VERIFIED_BY {
            let parent_is_verifiable = parent_name
                .and_then(|name| component_defs.get(name))
                .is_some_and(|def| def.verifiable);

            if !parent_is_verifiable {
                let context = match parent_name {
                    Some(name) => format!("under `{name}`"),
                    None => "at document root".into(),
                };
                findings.push(Finding::new(
                    RuleName::InvalidVerifiedByPlacement,
                    Some(doc_id.to_owned()),
                    format!(
                        "VerifiedBy in `{doc_id}` is placed {context}; \
                         it must be a direct child of a verifiable component (e.g. Criterion)"
                    ),
                    Some(comp.position),
                ));
            }
        }
        // Recurse into children
        walk_for_verified_by(
            doc_id,
            &comp.children,
            Some(&comp.name),
            component_defs,
            findings,
        );
    }
}

// ---------------------------------------------------------------------------
// check_{expected,rationale,alternative}_placement  (shared implementation)
// ---------------------------------------------------------------------------

/// Generic placement check: every occurrence of `child_name` must be a direct
/// child of `valid_parent`. Violations are reported under the given `rule`.
fn check_child_placement(
    docs: &[&SpecDocument],
    child_name: &str,
    valid_parent: &str,
    rule: RuleName,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        walk_for_child_placement(
            doc_id,
            &doc.components,
            None,
            child_name,
            valid_parent,
            rule,
            &mut findings,
        );
    }
    findings
}

fn walk_for_child_placement(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    parent_name: Option<&str>,
    child_name: &str,
    valid_parent: &str,
    rule: RuleName,
    findings: &mut Vec<Finding>,
) {
    for comp in components {
        if comp.name == child_name && parent_name != Some(valid_parent) {
            let context = match parent_name {
                Some(name) => format!("under `{name}`"),
                None => "at document root".into(),
            };
            findings.push(Finding::new(
                rule,
                Some(doc_id.to_owned()),
                format!(
                    "{child_name} in `{doc_id}` is placed {context}; \
                     it must be a direct child of {valid_parent}"
                ),
                Some(comp.position),
            ));
        }
        walk_for_child_placement(
            doc_id,
            &comp.children,
            Some(&comp.name),
            child_name,
            valid_parent,
            rule,
            findings,
        );
    }
}

/// Check that every `Expected` component is a direct child of an `Example`
/// component. `Expected` at document root or under any other component is a
/// structural error.
pub fn check_expected_placement(docs: &[&SpecDocument]) -> Vec<Finding> {
    check_child_placement(docs, EXPECTED, EXAMPLE, RuleName::InvalidExpectedPlacement)
}

/// Check that every `Rationale` component is a direct child of a `Decision`
/// component. `Rationale` at document root or under any other component is a
/// structural warning.
pub fn check_rationale_placement(docs: &[&SpecDocument]) -> Vec<Finding> {
    check_child_placement(
        docs,
        RATIONALE,
        DECISION,
        RuleName::InvalidRationalePlacement,
    )
}

/// Check that every `Alternative` component is a direct child of a `Decision`
/// component. `Alternative` at document root or under any other component is a
/// structural warning.
pub fn check_alternative_placement(docs: &[&SpecDocument]) -> Vec<Finding> {
    check_child_placement(
        docs,
        ALTERNATIVE,
        DECISION,
        RuleName::InvalidAlternativePlacement,
    )
}

// ---------------------------------------------------------------------------
// check_alternative_status
// ---------------------------------------------------------------------------

const RECOGNIZED_ALTERNATIVE_STATUSES: &[&str] = &["rejected", "deferred", "superseded"];

/// Check that every `Alternative` component's `status` attribute (if present)
/// is one of the recognized values: `rejected`, `deferred`, `superseded`.
pub fn check_alternative_status(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        for decision in crate::rules::find_components(&doc.components, DECISION) {
            for child in &decision.children {
                if child.name == ALTERNATIVE
                    && let Some(status) = child.attributes.get("status")
                    && !RECOGNIZED_ALTERNATIVE_STATUSES.contains(&status.as_str())
                {
                    findings.push(Finding::new(
                        RuleName::InvalidAlternativeStatus,
                        Some(doc_id.to_owned()),
                        format!(
                            "Alternative in `{doc_id}` has unrecognized status `{status}`; \
                             expected one of: {}",
                            RECOGNIZED_ALTERNATIVE_STATUSES.join(", ")
                        ),
                        Some(child.position),
                    ));
                }
            }
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_duplicate_rationale
// ---------------------------------------------------------------------------

/// Check that each `Decision` component has at most one `Rationale` child.
/// Emits a finding on each excess `Rationale` (the 2nd and beyond).
pub fn check_duplicate_rationale(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        for decision in crate::rules::find_components(&doc.components, DECISION) {
            let rationale_children: Vec<_> = decision
                .children
                .iter()
                .filter(|c| c.name == RATIONALE)
                .collect();
            for excess in rationale_children.iter().skip(1) {
                findings.push(Finding::new(
                    RuleName::DuplicateRationale,
                    Some(doc_id.to_owned()),
                    format!(
                        "Decision in `{doc_id}` has a duplicate Rationale child; \
                         only one Rationale per Decision is expected"
                    ),
                    Some(excess.position),
                ));
            }
        }
    }
    findings
}

// ---------------------------------------------------------------------------
// check_code_block_cardinality
// ---------------------------------------------------------------------------

/// Check that every `Example` component has exactly one code block, and every
/// `Expected` component has at most one code block.
pub fn check_code_block_cardinality(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        walk_for_code_block_cardinality(doc_id, &doc.components, &mut findings);
    }
    findings
}

fn walk_for_code_block_cardinality(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    findings: &mut Vec<Finding>,
) {
    for comp in components {
        if comp.name == EXAMPLE {
            let count = comp.code_blocks.len();
            if count != 1 {
                findings.push(Finding::new(
                    RuleName::InvalidCodeBlockCardinality,
                    Some(doc_id.to_owned()),
                    format!(
                        "Example in `{doc_id}` has {count} code block(s); \
                         it must have exactly 1"
                    ),
                    Some(comp.position),
                ));
            }
        } else if comp.name == EXPECTED {
            let count = comp.code_blocks.len();
            if count > 1 {
                findings.push(Finding::new(
                    RuleName::InvalidCodeBlockCardinality,
                    Some(doc_id.to_owned()),
                    format!(
                        "Expected in `{doc_id}` has {count} code block(s); \
                         it must have at most 1"
                    ),
                    Some(comp.position),
                ));
            }
        }
        walk_for_code_block_cardinality(doc_id, &comp.children, findings);
    }
}

// ---------------------------------------------------------------------------
// check_env_format
// ---------------------------------------------------------------------------

/// Check that every item in the `env` attribute of `Example` and `Expected`
/// components contains `=` (i.e. is in `KEY=VALUE` form).
pub fn check_env_format(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        walk_for_env_format(doc_id, &doc.components, &mut findings);
    }
    findings
}

fn walk_for_env_format(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    findings: &mut Vec<Finding>,
) {
    for comp in components {
        if (comp.name == EXAMPLE || comp.name == EXPECTED)
            && let Some(env_val) = comp.attributes.get("env")
        {
            for item in env_val.split(',') {
                let item = item.trim();
                if !item.is_empty() && !item.contains('=') {
                    findings.push(Finding::new(
                        RuleName::InvalidEnvFormat,
                        Some(doc_id.to_owned()),
                        format!(
                            "{} in `{doc_id}` has invalid env item `{item}`; \
                             each item must contain `=`",
                            comp.name
                        ),
                        Some(comp.position),
                    ));
                }
            }
        }
        walk_for_env_format(doc_id, &comp.children, findings);
    }
}

// ---------------------------------------------------------------------------
// Sequential ID parsing
// ---------------------------------------------------------------------------

/// Numeric key extracted from a sequential ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NumericKey {
    One(u32),
    Two(u32, u32),
}

/// A parsed sequential ID: prefix + numeric key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequentialId<'a> {
    pub prefix: &'a str,
    pub key: NumericKey,
}

/// Parse a component ID into a `SequentialId` if it matches the pattern
/// `prefix-N` or `prefix-N-M`, where prefix is one or more non-numeric
/// dash-separated segments and N, M are unsigned integers.
///
/// Returns `None` for non-sequential IDs.
pub(crate) fn parse_sequential_id(id: &str) -> Option<SequentialId<'_>> {
    let segments: Vec<&str> = id.split('-').collect();

    // Find the first numeric segment.
    let first_numeric = segments
        .iter()
        .position(|s| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty())?;

    // Must have at least one non-numeric prefix segment.
    if first_numeric == 0 {
        return None;
    }

    // Collect consecutive numeric segments after the prefix.
    let numeric_segments: Vec<&str> = segments[first_numeric..]
        .iter()
        .take_while(|s| s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty())
        .copied()
        .collect();

    // Only support 1 or 2 numeric segments.
    if numeric_segments.len() > 2 {
        return None;
    }

    // All segments after the prefix must be numeric (no trailing suffix).
    if first_numeric + numeric_segments.len() != segments.len() {
        return None;
    }

    let prefix_end = segments[..first_numeric]
        .iter()
        .map(|s| s.len())
        .sum::<usize>()
        + first_numeric
        - 1; // account for dashes between prefix segments
    let prefix = &id[..prefix_end];

    let n: u32 = numeric_segments[0].parse().ok()?;
    let key = if numeric_segments.len() == 2 {
        let m: u32 = numeric_segments[1].parse().ok()?;
        NumericKey::Two(n, m)
    } else {
        NumericKey::One(n)
    };

    Some(SequentialId { prefix, key })
}

// ---------------------------------------------------------------------------
// check_sequential_id_order
// ---------------------------------------------------------------------------

/// Check that sequentially-numbered components appear in ascending numeric
/// order by declaration position within each document.
pub fn check_sequential_id_order(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        check_sequential_order_at_level(doc_id, &doc.components, &mut findings);
    }
    findings
}

fn check_sequential_order_at_level(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    findings: &mut Vec<Finding>,
) {
    // Group sequential IDs by (prefix, arity), preserving declaration order.
    // One-level and two-level IDs in the same prefix are ordered independently.
    let mut last_key: std::collections::HashMap<(&str, u8), (NumericKey, &str)> =
        std::collections::HashMap::new();

    for comp in components {
        if is_referenceable(&comp.name)
            && let Some(id) = comp.attributes.get("id")
            && let Some(parsed) = parse_sequential_id(id)
        {
            let arity = match parsed.key {
                NumericKey::One(_) => 1,
                NumericKey::Two(_, _) => 2,
            };
            let group = (parsed.prefix, arity);
            if let Some((prev_key, prev_id)) = last_key.get(&group)
                && parsed.key <= *prev_key
            {
                findings.push(Finding::new(
                    RuleName::SequentialIdOrder,
                    Some(doc_id.to_owned()),
                    format!("`{id}` is declared after `{prev_id}` in document `{doc_id}`"),
                    Some(comp.position),
                ));
            }
            last_key.insert(group, (parsed.key, id));
        }
        // Always recurse into children (AcceptanceCriteria wraps Criterion).
        check_sequential_order_at_level(doc_id, &comp.children, findings);
    }
}

// ---------------------------------------------------------------------------
// check_sequential_id_gap
// ---------------------------------------------------------------------------

/// Check that sequentially-numbered components form contiguous sequences
/// within each prefix group.
pub fn check_sequential_id_gap(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = &doc.frontmatter.id;
        check_sequential_gap_at_level(doc_id, &doc.components, &mut findings);
    }
    findings
}

fn check_sequential_gap_at_level(
    doc_id: &str,
    components: &[supersigil_core::ExtractedComponent],
    findings: &mut Vec<Finding>,
) {
    // Collect sequential IDs grouped by prefix.
    let mut by_prefix: std::collections::HashMap<&str, Vec<NumericKey>> =
        std::collections::HashMap::new();

    collect_sequential_keys(components, &mut by_prefix);

    for (prefix, keys) in &by_prefix {
        check_contiguity(doc_id, prefix, keys, findings);
    }
}

fn collect_sequential_keys<'a>(
    components: &'a [supersigil_core::ExtractedComponent],
    by_prefix: &mut std::collections::HashMap<&'a str, Vec<NumericKey>>,
) {
    for comp in components {
        if is_referenceable(&comp.name)
            && let Some(id) = comp.attributes.get("id")
            && let Some(parsed) = parse_sequential_id(id)
        {
            by_prefix.entry(parsed.prefix).or_default().push(parsed.key);
        }
        // Always recurse into children.
        collect_sequential_keys(&comp.children, by_prefix);
    }
}

fn check_contiguity(doc_id: &str, prefix: &str, keys: &[NumericKey], findings: &mut Vec<Finding>) {
    if keys.is_empty() {
        return;
    }

    // Determine if we're dealing with single-level or two-level keys.
    let has_two_level = keys.iter().any(|k| matches!(k, NumericKey::Two(_, _)));
    let has_one_level = keys.iter().any(|k| matches!(k, NumericKey::One(_)));

    if has_two_level && !has_one_level {
        // Two-level: check first-level N contiguity, then per-N M contiguity.
        let mut by_n: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
        for key in keys {
            if let NumericKey::Two(n, m) = key {
                by_n.entry(*n).or_default().push(*m);
            }
        }

        // Check N contiguity.
        let n_values: Vec<u32> = by_n.keys().copied().collect();
        check_single_level_contiguity(doc_id, prefix, &n_values, true, findings);

        // Check M contiguity within each N.
        for (n, m_values) in &by_n {
            check_two_level_m_contiguity(doc_id, prefix, *n, m_values, findings);
        }
    } else if !has_two_level && has_one_level {
        // Single-level: check N contiguity.
        let n_values: Vec<u32> = keys
            .iter()
            .filter_map(|k| match k {
                NumericKey::One(n) => Some(*n),
                NumericKey::Two(_, _) => None,
            })
            .collect();
        check_single_level_contiguity(doc_id, prefix, &n_values, false, findings);
    }
    // Mixed one-level and two-level in same prefix group: skip (unusual).
}

/// Shared gap-detection logic for sequential ID contiguity checking.
///
/// Sorts and deduplicates `values`, then checks that every integer from 1 to
/// max is present. For each missing value, calls `format_id` to produce the
/// human-readable ID string for the gap message.
fn check_level_contiguity(
    doc_id: &str,
    values: &[u32],
    format_id: impl Fn(u32) -> String,
    findings: &mut Vec<Finding>,
) {
    if values.is_empty() {
        return;
    }
    let mut sorted: Vec<u32> = values.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let max = *sorted.last().unwrap();
    for expected in 1..=max {
        if sorted.binary_search(&expected).is_err() {
            let missing_id = format_id(expected);

            let msg = if expected == 1 {
                let first_present = format_id(sorted[0]);
                format!(
                    "gap in sequence: `{missing_id}` is missing (sequence starts at `{first_present}` in document `{doc_id}`)"
                )
            } else {
                let pred_id = format_id(expected - 1);
                if let Some(&succ) = sorted.iter().find(|&&v| v > expected) {
                    let succ_id = format_id(succ);
                    format!(
                        "gap in sequence: `{missing_id}` is missing (between `{pred_id}` and `{succ_id}` in document `{doc_id}`)"
                    )
                } else {
                    format!(
                        "gap in sequence: `{missing_id}` is missing (after `{pred_id}` in document `{doc_id}`)"
                    )
                }
            };
            findings.push(Finding::new(
                RuleName::SequentialIdGap,
                Some(doc_id.to_owned()),
                msg,
                None,
            ));
        }
    }
}

fn check_single_level_contiguity(
    doc_id: &str,
    prefix: &str,
    values: &[u32],
    is_outer_level: bool,
    findings: &mut Vec<Finding>,
) {
    check_level_contiguity(
        doc_id,
        values,
        |v| {
            if is_outer_level {
                format!("{prefix}-{v}-*")
            } else {
                format!("{prefix}-{v}")
            }
        },
        findings,
    );
}

fn check_two_level_m_contiguity(
    doc_id: &str,
    prefix: &str,
    n: u32,
    m_values: &[u32],
    findings: &mut Vec<Finding>,
) {
    check_level_contiguity(doc_id, m_values, |m| format!("{prefix}-{n}-{m}"), findings);
}

fn is_referenceable(name: &str) -> bool {
    name == "Criterion" || name == "Task"
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use supersigil_core::DocumentTypeDef;
    use supersigil_rust::verifies;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // check_required_components
    // -----------------------------------------------------------------------

    #[test]
    fn document_missing_required_component_emits_finding() {
        let mut config = test_config();
        config.documents.types.insert(
            "requirements".into(),
            DocumentTypeDef {
                status: vec!["draft".into()],
                required_components: vec!["AcceptanceCriteria".into()],
                description: None,
            },
        );
        let docs = vec![make_doc_typed(
            "req/auth",
            "requirements",
            Some("draft"),
            vec![],
        )];
        let graph = build_test_graph_with_config(docs, &config);
        let findings = check_required_components(&graph, &config);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::MissingRequiredComponent);
    }

    #[test]
    fn document_with_required_component_is_clean() {
        let mut config = test_config();
        config.documents.types.insert(
            "requirements".into(),
            DocumentTypeDef {
                status: vec!["draft".into()],
                required_components: vec!["AcceptanceCriteria".into()],
                description: None,
            },
        );
        let docs = vec![make_doc_typed(
            "req/auth",
            "requirements",
            Some("draft"),
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1", 10)],
                9,
            )],
        )];
        let graph = build_test_graph_with_config(docs, &config);
        let findings = check_required_components(&graph, &config);
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_id_pattern
    // -----------------------------------------------------------------------

    #[test]
    fn id_not_matching_pattern_emits_finding() {
        let mut config = test_config();
        config.id_pattern = Some(r"^(req|design|tasks)/".into());
        let docs = vec![make_doc("bad-id", vec![])];
        let graph = build_test_graph_with_config(docs, &config);
        let findings = check_id_pattern(&graph, &config);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidIdPattern);
    }

    #[test]
    fn id_matching_pattern_is_clean() {
        let mut config = test_config();
        config.id_pattern = Some(r"^(req|design|tasks)/".into());
        let docs = vec![make_doc("req/auth", vec![])];
        let graph = build_test_graph_with_config(docs, &config);
        let findings = check_id_pattern(&graph, &config);
        assert!(findings.is_empty());
    }

    #[test]
    fn no_id_pattern_means_no_findings() {
        let config = test_config();
        let docs = vec![make_doc("anything", vec![])];
        let graph = build_test_graph_with_config(docs, &config);
        let findings = check_id_pattern(&graph, &config);
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_isolated
    // -----------------------------------------------------------------------

    #[test]
    fn document_with_no_refs_emits_isolated() {
        let docs = vec![
            make_doc("lonely", vec![]),
            make_doc("connected-a", vec![make_implements("connected-b", 5)]),
            make_doc("connected-b", vec![]),
        ];
        let graph = build_test_graph(docs);
        let findings = check_isolated(&graph);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("lonely"));
    }

    #[test]
    fn depends_on_target_is_not_isolated() {
        // If A DependsOn B, then B has an incoming ref and should NOT be isolated.
        let docs = vec![
            make_doc("a", vec![make_depends_on("b", 5)]),
            make_doc("b", vec![]), // no outgoing refs, but has incoming DependsOn
        ];
        let graph = build_test_graph(docs);
        let findings = check_isolated(&graph);
        // Neither document should be isolated: A has outgoing, B has incoming DependsOn
        assert!(
            findings.is_empty(),
            "document 'b' should not be isolated (it is a DependsOn target), got: {findings:?}",
        );
    }

    #[test]
    fn document_with_outgoing_ref_is_not_isolated() {
        let docs = vec![
            make_doc("connected", vec![make_implements("other", 5)]),
            make_doc("other", vec![]),
        ];
        let graph = build_test_graph(docs);
        let findings = check_isolated(&graph);
        // "other" has incoming ref from "connected", so neither is isolated
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_orphan_tags
    // -----------------------------------------------------------------------

    #[test]
    fn tag_in_file_not_in_any_verified_by_emits_orphan() {
        let dir = TempDir::new().unwrap();
        write_test_file(&dir, "test.rs", "// supersigil: prop:orphaned-tag\n");
        let docs = [make_doc(
            "prop/auth",
            vec![make_verified_by_tag("prop:real-tag", 5)],
        )];
        let test_files = vec![dir.path().join("test.rs")];
        let tag_matches = crate::scan::scan_all_tags(&test_files);
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_orphan_tags(&doc_refs, &tag_matches);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::OrphanTestTag);
        assert!(findings[0].message.contains("prop:orphaned-tag"));
    }

    #[test]
    fn declared_tag_is_not_orphaned() {
        let dir = TempDir::new().unwrap();
        write_test_file(&dir, "test.rs", "// supersigil: prop:real-tag\n");
        let docs = [make_doc(
            "prop/auth",
            vec![make_verified_by_tag("prop:real-tag", 5)],
        )];
        let test_files = vec![dir.path().join("test.rs")];
        let tag_matches = crate::scan::scan_all_tags(&test_files);
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_orphan_tags(&doc_refs, &tag_matches);
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_verified_by_placement
    // -----------------------------------------------------------------------

    #[test]
    fn verified_by_under_criterion_is_valid() {
        let component_defs = supersigil_core::ComponentDefs::defaults();
        let docs = [make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "req-1",
                    make_verified_by_tag("auth:login", 11),
                    10,
                )],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_verified_by_placement(&doc_refs, &component_defs);
        assert!(
            findings.is_empty(),
            "VerifiedBy under Criterion should produce no structural errors, got: {findings:?}",
        );
    }

    #[test]
    fn verified_by_at_document_root_is_structural_error() {
        let component_defs = supersigil_core::ComponentDefs::defaults();
        let docs = [make_doc(
            "req/auth",
            vec![
                make_references("other/doc", 5),
                make_verified_by_tag("auth:login", 6),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_verified_by_placement(&doc_refs, &component_defs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidVerifiedByPlacement);
        assert!(
            findings[0].message.contains("verifiable"),
            "error message should mention 'verifiable', got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn verified_by_under_non_verifiable_component_is_structural_error() {
        let component_defs = supersigil_core::ComponentDefs::defaults();
        // AcceptanceCriteria is not verifiable, so VerifiedBy directly under it is invalid
        let docs = [make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_verified_by_tag("auth:login", 11)],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_verified_by_placement(&doc_refs, &component_defs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidVerifiedByPlacement);
    }

    #[test]
    fn nested_verified_by_under_verifiable_component_still_produces_evidence() {
        // This test verifies that evidence extraction (via explicit_evidence) still
        // works for VerifiedBy under Criterion. We check that the structural rule
        // does NOT flag it, which is the structural side of "still produces evidence".
        let component_defs = supersigil_core::ComponentDefs::defaults();

        let docs = [make_doc(
            "req/auth",
            vec![make_acceptance_criteria(
                vec![make_criterion_with_verified_by(
                    "req-1",
                    make_verified_by_glob("tests/**/*.rs", 11),
                    10,
                )],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_verified_by_placement(&doc_refs, &component_defs);
        assert!(
            findings.is_empty(),
            "VerifiedBy under Criterion should not produce structural errors, got: {findings:?}",
        );
    }

    #[test]
    fn multiple_verified_by_children_under_one_verifiable_component_are_additive() {
        // Multiple VerifiedBy under one Criterion should all be accepted
        let component_defs = supersigil_core::ComponentDefs::defaults();

        let criterion = supersigil_core::ExtractedComponent {
            name: "Criterion".into(),
            attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
            children: vec![
                make_verified_by_tag("auth:tag1", 11),
                make_verified_by_glob("tests/**/*.rs", 12),
                make_verified_by_tag("auth:tag2", 13),
            ],
            body_text: Some("criterion req-1".into()),
            code_blocks: vec![],
            position: pos(10),
        };
        let docs = [make_doc(
            "req/auth",
            vec![make_acceptance_criteria(vec![criterion], 9)],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_verified_by_placement(&doc_refs, &component_defs);
        assert!(
            findings.is_empty(),
            "multiple VerifiedBy under one Criterion should all be valid, got: {findings:?}",
        );
    }

    // -----------------------------------------------------------------------
    // check_expected_placement
    // -----------------------------------------------------------------------

    fn make_code_block() -> supersigil_core::CodeBlock {
        supersigil_core::CodeBlock {
            lang: Some("bash".into()),
            content: "echo hello".into(),
            content_offset: 0,
        }
    }

    fn make_example(
        children: Vec<supersigil_core::ExtractedComponent>,
        line: usize,
    ) -> supersigil_core::ExtractedComponent {
        supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::new(),
            children,
            body_text: None,
            code_blocks: vec![make_code_block()],
            position: pos(line),
        }
    }

    fn make_expected(line: usize) -> supersigil_core::ExtractedComponent {
        supersigil_core::ExtractedComponent {
            name: "Expected".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![],
            position: pos(line),
        }
    }

    #[test]
    fn expected_under_example_is_valid() {
        let expected = make_expected(11);
        let example = make_example(vec![expected], 10);
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_expected_placement(&doc_refs);
        assert!(
            findings.is_empty(),
            "Expected under Example should be valid, got: {findings:?}",
        );
    }

    #[test]
    fn expected_at_document_root_is_structural_error() {
        let expected = make_expected(5);
        let docs = [make_doc("ex/doc", vec![expected])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_expected_placement(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidExpectedPlacement);
        assert!(
            findings[0].message.contains("document root"),
            "message should mention document root, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn expected_under_non_example_component_is_structural_error() {
        // Expected nested inside AcceptanceCriteria (not Example)
        let expected = make_expected(11);
        let ac = make_acceptance_criteria(vec![expected], 9);
        let docs = [make_doc("ex/doc", vec![ac])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_expected_placement(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidExpectedPlacement);
        assert!(
            findings[0].message.contains("AcceptanceCriteria"),
            "message should mention parent name, got: {}",
            findings[0].message,
        );
    }

    // -----------------------------------------------------------------------
    // check_code_block_cardinality
    // -----------------------------------------------------------------------

    #[test]
    fn example_with_exactly_one_code_block_is_valid() {
        let example = supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_code_block_cardinality(&doc_refs);
        assert!(
            findings.is_empty(),
            "Example with 1 code block should be valid, got: {findings:?}"
        );
    }

    #[test]
    fn example_with_zero_code_blocks_emits_finding() {
        let example = supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_code_block_cardinality(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidCodeBlockCardinality);
        assert!(
            findings[0].message.contains("exactly 1"),
            "got: {}",
            findings[0].message
        );
    }

    #[test]
    fn example_with_two_code_blocks_emits_finding() {
        let example = supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block(), make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_code_block_cardinality(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidCodeBlockCardinality);
    }

    #[test]
    fn expected_with_zero_code_blocks_is_valid() {
        let expected = supersigil_core::ExtractedComponent {
            name: "Expected".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![expected])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_code_block_cardinality(&doc_refs);
        assert!(
            findings.is_empty(),
            "Expected with 0 code blocks should be valid, got: {findings:?}"
        );
    }

    #[test]
    fn expected_with_one_code_block_is_valid() {
        let expected = supersigil_core::ExtractedComponent {
            name: "Expected".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![expected])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_code_block_cardinality(&doc_refs);
        assert!(
            findings.is_empty(),
            "Expected with 1 code block should be valid, got: {findings:?}"
        );
    }

    #[test]
    fn expected_with_two_code_blocks_emits_finding() {
        let expected = supersigil_core::ExtractedComponent {
            name: "Expected".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block(), make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![expected])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_code_block_cardinality(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidCodeBlockCardinality);
        assert!(
            findings[0].message.contains("at most 1"),
            "got: {}",
            findings[0].message
        );
    }

    // -----------------------------------------------------------------------
    // check_env_format
    // -----------------------------------------------------------------------

    #[test]
    fn example_with_valid_env_is_clean() {
        let example = supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::from([("env".into(), "FOO=bar,BAZ=qux".into())]),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_env_format(&doc_refs);
        assert!(
            findings.is_empty(),
            "valid env items should not emit findings, got: {findings:?}"
        );
    }

    #[test]
    fn example_with_env_item_missing_equals_emits_finding() {
        let example = supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::from([("env".into(), "FOO=bar,BADITEM".into())]),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_env_format(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidEnvFormat);
        assert!(
            findings[0].message.contains("BADITEM"),
            "got: {}",
            findings[0].message
        );
    }

    #[test]
    fn expected_with_env_item_missing_equals_emits_finding() {
        let expected = supersigil_core::ExtractedComponent {
            name: "Expected".into(),
            attributes: std::collections::HashMap::from([("env".into(), "NOEQUALS".into())]),
            children: vec![],
            body_text: None,
            code_blocks: vec![],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![expected])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_env_format(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidEnvFormat);
    }

    #[test]
    fn component_without_env_attribute_is_clean() {
        let example = supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::new(),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_env_format(&doc_refs);
        assert!(
            findings.is_empty(),
            "no env attribute should not emit findings, got: {findings:?}"
        );
    }

    #[test]
    fn multiple_invalid_env_items_emit_multiple_findings() {
        let example = supersigil_core::ExtractedComponent {
            name: "Example".into(),
            attributes: std::collections::HashMap::from([(
                "env".into(),
                "NOEQ1,NOEQ2,VALID=ok".into(),
            )]),
            children: vec![],
            body_text: None,
            code_blocks: vec![make_code_block()],
            position: pos(5),
        };
        let docs = [make_doc("ex/doc", vec![example])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_env_format(&doc_refs);
        assert_eq!(findings.len(), 2);
    }

    // -----------------------------------------------------------------------
    // parse_sequential_id
    // -----------------------------------------------------------------------

    #[test]
    fn parse_single_level_id() {
        let parsed = parse_sequential_id("task-3").unwrap();
        assert_eq!(parsed.prefix, "task");
        assert_eq!(parsed.key, NumericKey::One(3));
    }

    #[test]
    fn parse_two_level_id() {
        let parsed = parse_sequential_id("req-1-2").unwrap();
        assert_eq!(parsed.prefix, "req");
        assert_eq!(parsed.key, NumericKey::Two(1, 2));
    }

    #[test]
    fn parse_multi_segment_prefix() {
        let parsed = parse_sequential_id("my-prefix-1-2").unwrap();
        assert_eq!(parsed.prefix, "my-prefix");
        assert_eq!(parsed.key, NumericKey::Two(1, 2));
    }

    #[test]
    fn non_sequential_semantic_id() {
        assert!(parse_sequential_id("login-success").is_none());
    }

    #[test]
    fn non_sequential_suffix_id() {
        assert!(parse_sequential_id("req-1-2-foo").is_none());
    }

    #[test]
    fn non_sequential_three_numeric_segments() {
        assert!(parse_sequential_id("req-1-2-3").is_none());
    }

    #[test]
    fn non_sequential_no_prefix() {
        assert!(parse_sequential_id("123").is_none());
    }

    #[test]
    fn non_sequential_single_segment() {
        assert!(parse_sequential_id("foo").is_none());
    }

    #[test]
    fn non_sequential_empty_string() {
        assert!(parse_sequential_id("").is_none());
    }

    #[test]
    fn numeric_key_ordering() {
        assert!(NumericKey::One(1) < NumericKey::One(2));
        assert!(NumericKey::Two(1, 1) < NumericKey::Two(1, 2));
        assert!(NumericKey::Two(1, 2) < NumericKey::Two(2, 1));
    }

    // -----------------------------------------------------------------------
    // check_sequential_id_order
    // -----------------------------------------------------------------------

    #[test]
    fn order_correct_criteria_no_findings() {
        let docs = [make_doc(
            "feature/req",
            vec![make_acceptance_criteria(
                vec![
                    make_criterion("req-1-1", 10),
                    make_criterion("req-1-2", 20),
                    make_criterion("req-2-1", 30),
                ],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_order(&doc_refs);
        assert!(
            findings.is_empty(),
            "correctly ordered IDs should produce no findings, got: {findings:?}"
        );
    }

    #[test]
    fn order_swapped_pair_emits_finding() {
        let docs = [make_doc(
            "feature/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1-2", 10), make_criterion("req-1-1", 20)],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_order(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::SequentialIdOrder);
        assert!(
            findings[0].message.contains("req-1-1"),
            "finding should name the out-of-order ID, got: {}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("req-1-2"),
            "finding should name the predecessor, got: {}",
            findings[0].message
        );
    }

    #[test]
    fn order_multiple_prefix_groups_independent() {
        // req-* in order, task-* out of order
        let docs = [make_doc(
            "feature/tasks",
            vec![
                make_criterion("req-1", 10),
                make_criterion("req-2", 20),
                make_task("task-2", 30),
                make_task("task-1", 40),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_order(&doc_refs);
        assert_eq!(
            findings.len(),
            1,
            "only task group should have findings, got: {findings:?}"
        );
        assert!(findings[0].message.contains("task-1"));
    }

    #[test]
    fn order_non_sequential_ids_skipped() {
        let docs = [make_doc(
            "feature/req",
            vec![
                make_criterion("login-success", 10),
                make_criterion("login-failure", 20),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_order(&doc_refs);
        assert!(
            findings.is_empty(),
            "non-sequential IDs should be skipped, got: {findings:?}"
        );
    }

    #[test]
    fn order_mixed_sequential_and_non_sequential() {
        let docs = [make_doc(
            "feature/req",
            vec![make_acceptance_criteria(
                vec![
                    make_criterion("req-1", 10),
                    make_criterion("login-check", 15),
                    make_criterion("req-2", 20),
                ],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_order(&doc_refs);
        assert!(
            findings.is_empty(),
            "non-sequential IDs should not interfere, got: {findings:?}"
        );
    }

    #[test]
    fn order_mixed_arity_no_false_positive() {
        // task-1 (One), task-1-1 (Two), task-2 (One) should not flag task-2
        let docs = [make_doc(
            "feature/tasks",
            vec![
                make_task("task-1", 10),
                make_task("task-1-1", 15),
                make_task("task-2", 20),
                make_task("task-4-1", 25),
                make_task("task-4-2", 30),
                make_task("task-5", 35),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_order(&doc_refs);
        assert!(
            findings.is_empty(),
            "mixed arity should not cause false positives, got: {findings:?}"
        );
    }

    #[test]
    fn order_tasks_correct_no_findings() {
        let docs = [make_doc(
            "feature/tasks",
            vec![
                make_task("task-1", 10),
                make_task("task-2", 20),
                make_task("task-3", 30),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_order(&doc_refs);
        assert!(findings.is_empty());
    }

    // -----------------------------------------------------------------------
    // check_sequential_id_gap
    // -----------------------------------------------------------------------

    #[test]
    fn gap_contiguous_sequence_no_findings() {
        let docs = [make_doc(
            "feature/tasks",
            vec![
                make_task("task-1", 10),
                make_task("task-2", 20),
                make_task("task-3", 30),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_gap(&doc_refs);
        assert!(
            findings.is_empty(),
            "contiguous sequence should produce no findings, got: {findings:?}"
        );
    }

    #[test]
    fn gap_missing_middle_element() {
        let docs = [make_doc(
            "feature/tasks",
            vec![make_task("task-1", 10), make_task("task-3", 30)],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_gap(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::SequentialIdGap);
        assert!(
            findings[0].message.contains("task-2"),
            "should name the missing ID, got: {}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("task-1"),
            "should reference predecessor, got: {}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("task-3"),
            "should reference successor, got: {}",
            findings[0].message
        );
    }

    #[test]
    fn gap_missing_first_element_leading_gap() {
        let docs = [make_doc(
            "feature/tasks",
            vec![make_task("task-2", 10), make_task("task-3", 20)],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_gap(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("task-1"),
            "should name the missing ID, got: {}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("starts at"),
            "leading gap should say 'starts at', got: {}",
            findings[0].message
        );
        assert!(
            findings[0].message.contains("task-2"),
            "should reference the first present ID, got: {}",
            findings[0].message
        );
    }

    #[test]
    fn gap_two_level_m_contiguity() {
        let docs = [make_doc(
            "feature/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1-1", 10), make_criterion("req-1-3", 30)],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_gap(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("req-1-2"),
            "should name the missing M-level ID, got: {}",
            findings[0].message
        );
    }

    #[test]
    fn gap_two_level_n_contiguity() {
        // Has req-1-1 and req-3-1, missing the entire req-2 group
        let docs = [make_doc(
            "feature/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("req-1-1", 10), make_criterion("req-3-1", 30)],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_gap(&doc_refs);
        assert!(
            findings.iter().any(|f| f.message.contains("req-2-*")),
            "should detect missing N-level group, got: {findings:?}"
        );
    }

    #[test]
    fn gap_non_sequential_ids_skipped() {
        let docs = [make_doc(
            "feature/req",
            vec![
                make_criterion("login-success", 10),
                make_criterion("login-failure", 20),
            ],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_gap(&doc_refs);
        assert!(
            findings.is_empty(),
            "non-sequential IDs should be skipped, got: {findings:?}"
        );
    }

    #[test]
    fn gap_two_level_contiguous_no_findings() {
        let docs = [make_doc(
            "feature/req",
            vec![make_acceptance_criteria(
                vec![
                    make_criterion("req-1-1", 10),
                    make_criterion("req-1-2", 20),
                    make_criterion("req-2-1", 30),
                    make_criterion("req-2-2", 40),
                ],
                9,
            )],
        )];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_sequential_id_gap(&doc_refs);
        assert!(
            findings.is_empty(),
            "contiguous two-level sequence should produce no findings, got: {findings:?}"
        );
    }

    // -----------------------------------------------------------------------
    // check_rationale_placement
    // -----------------------------------------------------------------------

    #[test]
    fn rationale_inside_decision_is_valid() {
        let decision = make_decision(vec![make_rationale(11)], 10);
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_rationale_placement(&doc_refs);
        assert!(
            findings.is_empty(),
            "Rationale inside Decision should be valid, got: {findings:?}",
        );
    }

    #[verifies("decision-components/req#req-2-2")]
    #[test]
    fn rationale_at_document_root_emits_finding() {
        let docs = [make_doc("adr/logging", vec![make_rationale(5)])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_rationale_placement(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidRationalePlacement);
        assert!(
            findings[0].message.contains("document root"),
            "message should mention document root, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn rationale_inside_non_decision_component_emits_finding() {
        // Rationale nested inside Criterion (not Decision)
        let criterion = supersigil_core::ExtractedComponent {
            name: "Criterion".into(),
            attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
            children: vec![make_rationale(11)],
            body_text: Some("criterion req-1".into()),
            code_blocks: vec![],
            position: pos(10),
        };
        let docs = [make_doc("adr/logging", vec![criterion])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_rationale_placement(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidRationalePlacement);
        assert!(
            findings[0].message.contains("Criterion"),
            "message should mention parent name, got: {}",
            findings[0].message,
        );
    }

    // -----------------------------------------------------------------------
    // check_alternative_placement
    // -----------------------------------------------------------------------

    #[test]
    fn alternative_inside_decision_is_valid() {
        let decision = make_decision(vec![make_alternative("alt-1", 11)], 10);
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_placement(&doc_refs);
        assert!(
            findings.is_empty(),
            "Alternative inside Decision should be valid, got: {findings:?}",
        );
    }

    #[verifies("decision-components/req#req-3-4")]
    #[test]
    fn alternative_at_document_root_emits_finding() {
        let docs = [make_doc("adr/logging", vec![make_alternative("alt-1", 5)])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_placement(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidAlternativePlacement);
        assert!(
            findings[0].message.contains("document root"),
            "message should mention document root, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn alternative_inside_non_decision_component_emits_finding() {
        // Alternative nested inside Criterion (not Decision)
        let criterion = supersigil_core::ExtractedComponent {
            name: "Criterion".into(),
            attributes: std::collections::HashMap::from([("id".into(), "req-1".into())]),
            children: vec![make_alternative("alt-1", 11)],
            body_text: Some("criterion req-1".into()),
            code_blocks: vec![],
            position: pos(10),
        };
        let docs = [make_doc("adr/logging", vec![criterion])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_placement(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidAlternativePlacement);
        assert!(
            findings[0].message.contains("Criterion"),
            "message should mention parent name, got: {}",
            findings[0].message,
        );
    }

    // -----------------------------------------------------------------------
    // check_duplicate_rationale
    // -----------------------------------------------------------------------

    #[test]
    fn decision_with_zero_rationale_no_finding() {
        let decision = make_decision(vec![], 10);
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_duplicate_rationale(&doc_refs);
        assert!(
            findings.is_empty(),
            "Decision with zero Rationale children should produce no findings, got: {findings:?}",
        );
    }

    #[test]
    fn decision_with_one_rationale_no_finding() {
        let decision = make_decision(vec![make_rationale(11)], 10);
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_duplicate_rationale(&doc_refs);
        assert!(
            findings.is_empty(),
            "Decision with one Rationale child should produce no findings, got: {findings:?}",
        );
    }

    #[verifies("decision-components/req#req-2-3")]
    #[test]
    fn decision_with_two_rationale_emits_finding_on_second() {
        let decision = make_decision(vec![make_rationale(11), make_rationale(12)], 10);
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_duplicate_rationale(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::DuplicateRationale);
        // Finding should be on the second Rationale (line 12)
        assert_eq!(
            findings[0].position.as_ref().map(|p| p.line),
            Some(12),
            "finding should point to the second Rationale",
        );
        assert!(
            findings[0].message.contains("duplicate"),
            "message should mention duplicate, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn duplicate_rationale_draft_gating() {
        let decision = make_decision(vec![make_rationale(11), make_rationale(12)], 10);
        let docs = vec![make_doc_with_status("adr/logging", "draft", vec![decision])];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = crate::VerifyOptions::default();
        let ag = crate::artifact_graph::ArtifactGraph::empty(&graph);
        let report =
            crate::verify(&graph, &config, std::path::Path::new("/tmp"), &options, &ag).unwrap();
        for finding in &report.findings {
            if finding.rule == RuleName::DuplicateRationale {
                assert_eq!(
                    finding.effective_severity,
                    crate::report::ReportSeverity::Info,
                    "draft doc duplicate rationale findings should be Info, got {:?}",
                    finding.effective_severity,
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // check_alternative_status
    // -----------------------------------------------------------------------

    #[test]
    fn alternative_with_status_rejected_no_finding() {
        let decision = make_decision(
            vec![make_alternative_with_status("alt-1", "rejected", 11)],
            10,
        );
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_status(&doc_refs);
        assert!(
            findings.is_empty(),
            "Alternative with status='rejected' should produce no findings, got: {findings:?}",
        );
    }

    #[test]
    fn alternative_with_status_deferred_no_finding() {
        let decision = make_decision(
            vec![make_alternative_with_status("alt-1", "deferred", 11)],
            10,
        );
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_status(&doc_refs);
        assert!(
            findings.is_empty(),
            "Alternative with status='deferred' should produce no findings, got: {findings:?}",
        );
    }

    #[test]
    fn alternative_with_status_superseded_no_finding() {
        let decision = make_decision(
            vec![make_alternative_with_status("alt-1", "superseded", 11)],
            10,
        );
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_status(&doc_refs);
        assert!(
            findings.is_empty(),
            "Alternative with status='superseded' should produce no findings, got: {findings:?}",
        );
    }

    #[verifies("decision-components/req#req-3-2")]
    #[test]
    fn alternative_with_status_accepted_emits_finding() {
        let decision = make_decision(
            vec![make_alternative_with_status("alt-1", "accepted", 11)],
            10,
        );
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_status(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidAlternativeStatus);
        assert!(
            findings[0].message.contains("accepted"),
            "message should mention the invalid status, got: {}",
            findings[0].message,
        );
    }

    #[test]
    fn alternative_with_empty_status_emits_finding() {
        let decision = make_decision(vec![make_alternative_with_status("alt-1", "", 11)], 10);
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_status(&doc_refs);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RuleName::InvalidAlternativeStatus);
    }

    #[test]
    fn alternative_without_status_attribute_no_finding() {
        // Alternative without any status attribute should not fire this rule
        let decision = make_decision(vec![make_alternative("alt-1", 11)], 10);
        let docs = [make_doc("adr/logging", vec![decision])];
        let doc_refs: Vec<&_> = docs.iter().collect();
        let findings = check_alternative_status(&doc_refs);
        assert!(
            findings.is_empty(),
            "Alternative without status attribute should produce no findings, got: {findings:?}",
        );
    }

    #[verifies("decision-components/req#req-3-3")]
    #[test]
    fn alternative_status_default_severity_is_warning() {
        assert_eq!(
            RuleName::InvalidAlternativeStatus.default_severity(),
            crate::report::ReportSeverity::Warning,
        );
    }

    #[test]
    fn alternative_status_draft_gating() {
        let decision = make_decision(
            vec![make_alternative_with_status("alt-1", "accepted", 11)],
            10,
        );
        let docs = vec![make_doc_with_status("adr/logging", "draft", vec![decision])];
        let graph = build_test_graph(docs);
        let config = test_config();
        let options = crate::VerifyOptions::default();
        let ag = crate::artifact_graph::ArtifactGraph::empty(&graph);
        let report =
            crate::verify(&graph, &config, std::path::Path::new("/tmp"), &options, &ag).unwrap();
        for finding in &report.findings {
            if finding.rule == RuleName::InvalidAlternativeStatus {
                assert_eq!(
                    finding.effective_severity,
                    crate::report::ReportSeverity::Info,
                    "draft doc alternative status findings should be Info, got {:?}",
                    finding.effective_severity,
                );
            }
        }
    }
}
