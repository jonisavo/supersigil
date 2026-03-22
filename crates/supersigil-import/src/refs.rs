use std::collections::{HashMap, HashSet};

use regex::Regex;
use std::sync::LazyLock;

use crate::parse::RawRef;
use crate::parse::requirements::{ParsedRequirement, ParsedRequirements};

static REF_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\w+)\.(\w+)$").expect("valid regex"));

static RANGE_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\w+)\.(\w+)[–\-](\w+)\.(\w+)$").expect("valid regex"));

/// Parse a requirement reference string into individual refs.
///
/// Handles: `Requirements X.Y`, `X.Y, Z.W`, ranges `X.Y–X.Z`.
/// Returns parsed refs and any ambiguity markers for unparseable portions.
///
/// # Panics
///
/// Does not panic under normal usage.
#[must_use]
pub fn parse_requirement_refs(input: &str) -> (Vec<RawRef>, Vec<String>) {
    let mut refs = Vec::new();
    let mut markers = Vec::new();

    // Strip optional "Requirements" prefix
    let body = input
        .strip_prefix("Requirements")
        .map_or(input, str::trim_start)
        .trim();

    if body.is_empty() {
        markers.push(format!(
            "<!-- TODO(supersigil-import): Empty reference string in '{input}' -->"
        ));
        return (refs, markers);
    }

    for token in body.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        // Try range pattern first (X.Y–X.Z or X.Y-X.Z)
        if let Some(range_refs) = try_parse_range(token, &mut markers) {
            refs.extend(range_refs);
        } else if let Some(cap) = REF_PATTERN.captures(token) {
            // Single ref X.Y (anchored regex ensures full-token match)
            let req_num = cap[1].to_string();
            let crit_idx = cap[2].to_string();

            refs.push(RawRef {
                requirement_number: req_num,
                criterion_index: crit_idx,
            });
        } else {
            // Unparseable token
            markers.push(format!(
                "<!-- TODO(supersigil-import): Could not parse reference token '{token}' -->"
            ));
        }
    }

    (refs, markers)
}

/// Try to parse a token as a range `X.Y–X.Z` or `X.Y-X.Z`.
/// Returns `Some(expanded_refs)` on success, `None` if not a range.
fn try_parse_range(token: &str, markers: &mut Vec<String>) -> Option<Vec<RawRef>> {
    let cap = RANGE_PATTERN.captures(token)?;

    let req_start = &cap[1];
    let idx_start = &cap[2];
    let req_end = &cap[3];
    let idx_end = &cap[4];

    // Both start and end indices must be purely numeric for expansion
    let start_num: Option<u32> = idx_start.parse().ok();
    let end_num: Option<u32> = idx_end.parse().ok();

    match (start_num, end_num) {
        (Some(s), Some(e)) if s <= e => {
            // Numeric range — expand
            let req_num = req_start.to_string();
            let expanded: Vec<RawRef> = (s..=e)
                .map(|i| RawRef {
                    requirement_number: req_num.clone(),
                    criterion_index: i.to_string(),
                })
                .collect();

            // If the requirement numbers differ, that's unusual but we still expand
            // using the start requirement number (the range pattern matched)
            if req_start != req_end {
                markers.push(format!(
                    "<!-- TODO(supersigil-import): Range has different requirement numbers: \
                     '{req_start}' vs '{req_end}' in '{token}' -->"
                ));
            }

            Some(expanded)
        }
        (Some(s), Some(e)) if s > e => {
            // Reversed range
            markers.push(format!(
                "<!-- TODO(supersigil-import): Range has start > end: '{token}' -->"
            ));
            Some(vec![])
        }
        _ => {
            // Non-numeric indices in range
            markers.push(format!(
                "<!-- TODO(supersigil-import): Non-numeric range indices in '{token}', \
                 cannot expand -->"
            ));
            Some(vec![])
        }
    }
}

/// Pre-built index of requirements by number for efficient repeated lookups.
#[derive(Debug)]
pub struct RequirementIndex<'a> {
    by_number: HashMap<&'a str, &'a ParsedRequirement>,
    criterion_indices: HashMap<&'a str, HashSet<&'a str>>,
}

impl<'a> RequirementIndex<'a> {
    /// Build an index from parsed requirements. Call once per feature, then
    /// pass to `resolve_refs` for each set of refs.
    #[must_use]
    pub fn new(requirements: &'a ParsedRequirements) -> Self {
        let by_number = requirements
            .requirements
            .iter()
            .map(|r| (r.number.as_str(), r))
            .collect();
        let criterion_indices = requirements
            .requirements
            .iter()
            .map(|r| {
                let indices = r.criteria.iter().map(|c| c.index.as_str()).collect();
                (r.number.as_str(), indices)
            })
            .collect();
        Self {
            by_number,
            criterion_indices,
        }
    }

    #[must_use]
    fn has_criterion(&self, requirement_number: &str, criterion_index: &str) -> bool {
        self.criterion_indices
            .get(requirement_number)
            .is_some_and(|indices| indices.contains(criterion_index))
    }
}

/// Resolve a list of `RawRef`s against the requirement index to produce
/// criterion ref strings for spec document output.
///
/// Each resolvable ref becomes `{doc_id_base}#req-{X}-{Y}`. Unresolvable refs
/// (requirement number or criterion index not found) produce an ambiguity marker
/// and are excluded from the resolved list.
///
/// Returns `(resolved_refs, ambiguity_markers)`.
#[must_use]
pub fn resolve_refs(
    raw_refs: &[RawRef],
    index: &RequirementIndex<'_>,
    doc_id_base: &str,
) -> (Vec<String>, Vec<String>) {
    let mut resolved = Vec::new();
    let mut markers = Vec::new();

    for raw in raw_refs {
        let req = index
            .by_number
            .get(raw.requirement_number.as_str())
            .copied();

        let Some(req) = req else {
            markers.push(format!(
                "<!-- TODO(supersigil-import): Could not resolve reference \
                 'Requirements {}.{}' to a criterion ID — requirement {} not found -->",
                raw.requirement_number, raw.criterion_index, raw.requirement_number
            ));
            continue;
        };

        let has_criterion = index.has_criterion(&req.number, raw.criterion_index.as_str());

        if has_criterion {
            let crit_id =
                crate::ids::make_criterion_id(&raw.requirement_number, &raw.criterion_index);
            resolved.push(format!("{doc_id_base}#{crit_id}"));
        } else {
            markers.push(format!(
                "<!-- TODO(supersigil-import): Could not resolve reference \
                 'Requirements {}.{}' to a criterion ID — criterion index {} \
                 not found in requirement {} -->",
                raw.requirement_number,
                raw.criterion_index,
                raw.criterion_index,
                raw.requirement_number
            ));
        }
    }

    (resolved, markers)
}
