use std::collections::{BTreeMap, HashMap, HashSet};

use supersigil_core::{
    ALTERNATIVE, CRITERION, ComponentDefs, Config, DECISION, DEPENDS_ON, DocumentGraph, EXAMPLE,
    EXPECTED, ExtractedComponent, IMPLEMENTS, RATIONALE, REFERENCES, SourcePosition, SpecDocument,
    TASK, VERIFIED_BY,
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
    let task_level_connected = collect_task_level_connected_docs(graph);
    let mut findings = Vec::new();
    for (doc_id, doc) in graph.documents() {
        // Check outgoing refs (document has ref components)
        let has_outgoing = [REFERENCES, IMPLEMENTS, DEPENDS_ON]
            .iter()
            .any(|name| has_component(&doc.components, name));

        // Check incoming refs (other docs reference this one)
        let has_incoming = !graph.references(doc_id, None).is_empty()
            || !graph.implements(doc_id).is_empty()
            || !graph.depends_on(doc_id).is_empty();

        let has_task_level_refs = task_level_connected.contains(doc_id);

        if !has_outgoing && !has_incoming && !has_task_level_refs {
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

/// Collect all document IDs that participate in cross-document task-level
/// implements relationships (both source and target).
fn collect_task_level_connected_docs(graph: &DocumentGraph) -> HashSet<&str> {
    let mut connected = HashSet::new();

    for (doc_id, _) in graph.documents() {
        let Some(task_order) = graph.task_order(doc_id) else {
            continue;
        };

        for task_id in task_order {
            let Some(implementations) = graph.task_implements(doc_id, task_id) else {
                continue;
            };

            for (target_doc, _) in implementations {
                if target_doc != doc_id {
                    connected.insert(doc_id);
                    connected.insert(target_doc.as_str());
                }
            }
        }
    }

    connected
}

// ---------------------------------------------------------------------------
// check_orphan_tags
// ---------------------------------------------------------------------------

/// Check pre-scanned tag matches for tags not declared in any `VerifiedBy` component.
///
/// `tag_matches` should be pre-computed via [`crate::scan::scan_all_tags`].
pub fn check_orphan_tags(docs: &[&SpecDocument], tag_matches: &[TagMatch]) -> Vec<Finding> {
    // Collect declared tags from VerifiedBy components
    let mut declared_tags: HashSet<&str> = HashSet::new();
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
    let mut seen_orphans: HashSet<&str> = HashSet::new();
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

fn visit_components<'a, F>(
    components: &'a [ExtractedComponent],
    parent_name: Option<&'a str>,
    visit: &mut F,
) where
    F: FnMut(&'a ExtractedComponent, Option<&'a str>),
{
    for component in components {
        visit(component, parent_name);
        visit_components(&component.children, Some(component.name.as_str()), visit);
    }
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
        let doc_id = doc.frontmatter.id.as_str();
        let mut visit = |component: &ExtractedComponent, parent_name: Option<&str>| {
            if component.name != VERIFIED_BY {
                return;
            }

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
                    Some(component.position),
                ));
            }
        };
        visit_components(&doc.components, None, &mut visit);
    }
    findings
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
        let doc_id = doc.frontmatter.id.as_str();
        let mut visit = |component: &ExtractedComponent, parent_name: Option<&str>| {
            if component.name != child_name || parent_name == Some(valid_parent) {
                return;
            }

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
                Some(component.position),
            ));
        };
        visit_components(&doc.components, None, &mut visit);
    }
    findings
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
        let doc_id = doc.frontmatter.id.as_str();
        let mut visit = |component: &ExtractedComponent, _parent_name: Option<&str>| match component
            .name
            .as_str()
        {
            EXAMPLE => {
                let count = component.code_blocks.len();
                if count != 1 {
                    findings.push(Finding::new(
                        RuleName::InvalidCodeBlockCardinality,
                        Some(doc_id.to_owned()),
                        format!(
                            "Example in `{doc_id}` has {count} code block(s); \
                                 it must have exactly 1"
                        ),
                        Some(component.position),
                    ));
                }
            }
            EXPECTED => {
                let count = component.code_blocks.len();
                if count > 1 {
                    findings.push(Finding::new(
                        RuleName::InvalidCodeBlockCardinality,
                        Some(doc_id.to_owned()),
                        format!(
                            "Expected in `{doc_id}` has {count} code block(s); \
                                 it must have at most 1"
                        ),
                        Some(component.position),
                    ));
                }
            }
            _ => {}
        };
        visit_components(&doc.components, None, &mut visit);
    }
    findings
}

// ---------------------------------------------------------------------------
// check_env_format
// ---------------------------------------------------------------------------

/// Check that every item in the `env` attribute of `Example` and `Expected`
/// components contains `=` (i.e. is in `KEY=VALUE` form).
pub fn check_env_format(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = doc.frontmatter.id.as_str();
        let mut visit = |component: &ExtractedComponent, _parent_name: Option<&str>| {
            if (component.name != EXAMPLE && component.name != EXPECTED)
                || !component.attributes.contains_key("env")
            {
                return;
            }

            let env_val = component
                .attributes
                .get("env")
                .expect("checked env attribute above");
            for item in env_val.split(',') {
                let item = item.trim();
                if !item.is_empty() && !item.contains('=') {
                    findings.push(Finding::new(
                        RuleName::InvalidEnvFormat,
                        Some(doc_id.to_owned()),
                        format!(
                            "{} in `{doc_id}` has invalid env item `{item}`; \
                             each item must contain `=`",
                            component.name
                        ),
                        Some(component.position),
                    ));
                }
            }
        };
        visit_components(&doc.components, None, &mut visit);
    }
    findings
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

impl NumericKey {
    const fn arity(self) -> u8 {
        match self {
            Self::One(_) => 1,
            Self::Two(_, _) => 2,
        }
    }
}

/// A parsed sequential ID: prefix + numeric key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SequentialId<'a> {
    pub prefix: &'a str,
    pub key: NumericKey,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SequentialOccurrence<'a> {
    id: &'a str,
    prefix: &'a str,
    key: NumericKey,
    position: SourcePosition,
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
#[cfg(test)]
fn check_sequential_id_order(docs: &[&SpecDocument]) -> Vec<Finding> {
    let (findings, _) = check_sequential_ids(docs);
    findings
}

pub(crate) fn check_sequential_ids(docs: &[&SpecDocument]) -> (Vec<Finding>, Vec<Finding>) {
    let mut order_findings = Vec::new();
    let mut gap_findings = Vec::new();

    for doc in docs {
        let doc_id = doc.frontmatter.id.as_str();
        let occurrences = collect_sequential_occurrences(&doc.components);
        collect_sequential_order_findings(doc_id, &occurrences, &mut order_findings);
        collect_sequential_gap_findings(doc_id, &occurrences, &mut gap_findings);
    }

    (order_findings, gap_findings)
}

fn collect_sequential_occurrences<'a>(
    components: &'a [ExtractedComponent],
) -> Vec<SequentialOccurrence<'a>> {
    let mut occurrences = Vec::new();
    let mut visit = |component: &'a ExtractedComponent, _parent_name: Option<&'a str>| {
        if is_referenceable(&component.name)
            && let Some(id) = component.attributes.get("id")
            && let Some(parsed) = parse_sequential_id(id)
        {
            occurrences.push(SequentialOccurrence {
                id: id.as_str(),
                prefix: parsed.prefix,
                key: parsed.key,
                position: component.position,
            });
        }
    };
    visit_components(components, None, &mut visit);
    occurrences
}

fn collect_sequential_order_findings(
    doc_id: &str,
    occurrences: &[SequentialOccurrence<'_>],
    findings: &mut Vec<Finding>,
) {
    // Group sequential IDs by (prefix, arity), preserving declaration order.
    // One-level and two-level IDs in the same prefix are ordered independently.
    let mut last_key: HashMap<(&str, u8), (NumericKey, &str)> = HashMap::new();

    for occurrence in occurrences {
        let group = (occurrence.prefix, occurrence.key.arity());
        if let Some((prev_key, prev_id)) = last_key.get(&group)
            && occurrence.key <= *prev_key
        {
            findings.push(Finding::new(
                RuleName::SequentialIdOrder,
                Some(doc_id.to_owned()),
                format!(
                    "`{}` is declared after `{prev_id}` in document `{doc_id}`",
                    occurrence.id
                ),
                Some(occurrence.position),
            ));
        }
        last_key.insert(group, (occurrence.key, occurrence.id));
    }
}

// ---------------------------------------------------------------------------
// check_sequential_id_gap
// ---------------------------------------------------------------------------

/// Check that sequentially-numbered components form contiguous sequences
/// within each prefix group.
#[cfg(test)]
fn check_sequential_id_gap(docs: &[&SpecDocument]) -> Vec<Finding> {
    let (_, findings) = check_sequential_ids(docs);
    findings
}

fn collect_sequential_gap_findings(
    doc_id: &str,
    occurrences: &[SequentialOccurrence<'_>],
    findings: &mut Vec<Finding>,
) {
    let mut by_prefix: HashMap<&str, Vec<NumericKey>> = HashMap::new();

    for occurrence in occurrences {
        by_prefix
            .entry(occurrence.prefix)
            .or_default()
            .push(occurrence.key);
    }

    for (prefix, keys) in &by_prefix {
        check_contiguity(doc_id, prefix, keys, findings);
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
        let mut by_n: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
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

// ---------------------------------------------------------------------------
// check_expected_cardinality
// ---------------------------------------------------------------------------

/// Check that every `Example` component has at most one `Expected` child.
/// Examples with 2+ `Expected` children are a structural error.
pub fn check_expected_cardinality(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = doc.frontmatter.id.as_str();
        let mut visit = |component: &ExtractedComponent, _parent_name: Option<&str>| {
            if component.name != EXAMPLE {
                return;
            }
            let count = component
                .children
                .iter()
                .filter(|c| c.name == EXPECTED)
                .count();
            if count > 1 {
                findings.push(Finding::new(
                    RuleName::MultipleExpectedChildren,
                    Some(doc_id.to_owned()),
                    format!(
                        "Example in `{doc_id}` has {count} Expected children; \
                         it must have at most 1"
                    ),
                    Some(component.position),
                ));
            }
        };
        visit_components(&doc.components, None, &mut visit);
    }
    findings
}

// ---------------------------------------------------------------------------
// check_inline_example_lang
// ---------------------------------------------------------------------------

/// Check that `Example` components with inline code content (code block with
/// `lang: None`) have a `lang` attribute on the component itself.
pub fn check_inline_example_lang(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = doc.frontmatter.id.as_str();
        let mut visit = |component: &ExtractedComponent, _parent_name: Option<&str>| {
            if component.name != EXAMPLE {
                return;
            }
            // Check the first code block's lang field
            let has_fence_lang = component
                .code_blocks
                .first()
                .and_then(|cb| cb.lang.as_ref())
                .is_some();
            if has_fence_lang {
                // Code block has a language from the fence info string — no error
                return;
            }
            // No code block at all means nothing to check (cardinality rule handles that)
            if component.code_blocks.is_empty() {
                return;
            }
            // Code block exists but has lang: None — check for attribute
            let has_lang_attr = component.attributes.contains_key("lang");
            if !has_lang_attr {
                findings.push(Finding::new(
                    RuleName::InlineExampleWithoutLang,
                    Some(doc_id.to_owned()),
                    format!(
                        "Example in `{doc_id}` has inline code without a language; \
                         add a `lang` attribute or use a fenced code block with a language tag"
                    ),
                    Some(component.position),
                ));
            }
        };
        visit_components(&doc.components, None, &mut visit);
    }
    findings
}

// ---------------------------------------------------------------------------
// check_code_ref_conflicts
// ---------------------------------------------------------------------------

/// Surface non-fatal code-ref parse warnings (orphan refs, duplicate refs,
/// dual-source conflicts) as verification findings so they are visible in
/// `supersigil verify` output, not only in `supersigil lint` / LSP.
pub fn check_code_ref_conflicts(docs: &[&SpecDocument]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for doc in docs {
        let doc_id = doc.frontmatter.id.as_str();
        for warning in &doc.warnings {
            findings.push(Finding::new(
                RuleName::CodeRefConflict,
                Some(doc_id.to_owned()),
                warning.to_string(),
                None,
            ));
        }
    }
    findings
}

fn is_referenceable(name: &str) -> bool {
    name == CRITERION || name == TASK
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests;
