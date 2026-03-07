use std::fmt::Write;

use crate::emit::emit_front_matter;
use crate::ids::{deduplicate_ids, make_criterion_id};
use crate::parse::requirements::ParsedRequirements;

/// Emit a requirements MDX document from parsed Kiro requirements.
///
/// Returns `(mdx_content, ambiguity_count)`.
#[must_use]
#[allow(
    clippy::missing_panics_doc,
    reason = "internal invariant: deduped IDs align with criteria"
)]
pub fn emit_requirements_mdx(
    parsed: &ParsedRequirements,
    doc_id: &str,
    feature_title: &str,
) -> (String, usize) {
    let mut out = String::new();
    let mut ambiguity_count = 0;

    emit_front_matter(&mut out, doc_id, "requirements", feature_title);

    // Introduction prose
    if !parsed.introduction.trim().is_empty() {
        out.push_str(&parsed.introduction);
        out.push_str("\n\n");
    }

    // Glossary (if present)
    if let Some(glossary) = parsed.glossary.as_ref().filter(|g| !g.trim().is_empty()) {
        out.push_str(glossary);
        out.push_str("\n\n");
    }

    // Collect all criterion IDs for deduplication (Req 3.2, 3.3)
    let raw_ids: Vec<String> = parsed
        .requirements
        .iter()
        .flat_map(|req| {
            req.criteria
                .iter()
                .map(move |c| make_criterion_id(&req.number, &c.index))
        })
        .collect();
    let (deduped_ids, dedup_markers) = deduplicate_ids(&raw_ids);
    ambiguity_count += dedup_markers.len();
    let mut id_iter = deduped_ids.iter();

    // Per-requirement sections
    for req in &parsed.requirements {
        // Section heading
        if let Some(ref title) = req.title {
            let _ = writeln!(out, "## Requirement {}: {title}", req.number);
        } else {
            let _ = writeln!(out, "## Requirement {}", req.number);
        }
        out.push('\n');
        // User story
        if let Some(ref story) = req.user_story {
            out.push_str(story);
            out.push_str("\n\n");
        }

        // Extra prose before/after criteria
        for prose in &req.extra_prose {
            if !prose.trim().is_empty() {
                out.push_str(prose);
                out.push_str("\n\n");
            }
        }

        // AcceptanceCriteria block
        if !req.criteria.is_empty() {
            let _ = writeln!(out, "<AcceptanceCriteria>");
            for criterion in &req.criteria {
                let crit_id = id_iter.next().expect("deduped IDs aligned with criteria");
                let _ = writeln!(out, "  <Criterion id=\"{crit_id}\">");
                let _ = writeln!(out, "    {}", criterion.text);
                let _ = writeln!(out, "  </Criterion>");
            }
            let _ = writeln!(out, "</AcceptanceCriteria>");
            out.push('\n');
        }
    }

    // Append dedup markers at end of document
    for marker in &dedup_markers {
        let _ = writeln!(out, "{marker}");
    }

    (out, ambiguity_count)
}
