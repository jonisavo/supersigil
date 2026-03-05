use std::collections::HashMap;
use std::fmt::Write;

use crate::emit::emit_front_matter;
use crate::parse::design::{DesignBlock, ParsedDesign};

/// Key for mapping a `ValidatesLine` to resolved refs by structural position.
pub type ValidatesKey = (usize, usize);

/// Emit a design MDX document from parsed Kiro design.
///
/// `req_doc_id` is `Some(...)` when the same feature has a `requirements.md`,
/// enabling the `<Implements>` component. When `None`, an ambiguity marker is
/// emitted instead.
///
/// `resolved_validates` maps `(section_index, block_index)` pairs to resolved
/// criterion ref strings (produced by `resolve_refs`). `ambiguity_markers` are
/// pre-computed markers from the resolution phase.
///
/// Returns `(mdx_content, ambiguity_count)`.
#[must_use]
#[allow(clippy::implicit_hasher, reason = "public API always uses std HashMap")]
pub fn emit_design_mdx(
    parsed: &ParsedDesign,
    doc_id: &str,
    req_doc_id: Option<&str>,
    resolved_validates: &HashMap<ValidatesKey, Vec<String>>,
    feature_title: &str,
    ambiguity_markers: &[String],
) -> (String, usize) {
    let mut out = String::new();
    let mut ambiguity_count = ambiguity_markers.len();

    emit_front_matter(&mut out, doc_id, "design", feature_title);

    // <Implements> or ambiguity marker
    if let Some(req_id) = req_doc_id {
        let _ = writeln!(out, "<Implements refs=\"{req_id}\" />");
        out.push('\n');
    } else {
        let marker = "<!-- TODO(supersigil-import): No requirements document found for this \
                       feature; cannot emit <Implements> component -->";
        let _ = writeln!(out, "{marker}");
        out.push('\n');
        ambiguity_count += 1;
    }

    // Emit sections
    for (section_idx, section) in parsed.sections.iter().enumerate() {
        // Section heading (skip for synthetic preamble sections at level 0)
        if section.level > 0 {
            let hashes = "#".repeat(section.level as usize);
            let _ = writeln!(out, "{hashes} {}", section.heading);
            out.push('\n');
        }

        for (block_idx, block) in section.content.iter().enumerate() {
            match block {
                DesignBlock::Prose(text) => {
                    // Count any ambiguity markers embedded in prose (e.g., from
                    // non-requirement Validates targets converted to prose during parsing).
                    ambiguity_count += text.matches("<!-- TODO(supersigil-import):").count();
                    out.push_str(text);
                    out.push_str("\n\n");
                }
                DesignBlock::CodeBlock { language, content } => {
                    let lang = language.as_deref().unwrap_or("");
                    let _ = writeln!(out, "```{lang}");
                    let _ = writeln!(out, "{content}");
                    let _ = writeln!(out, "```");
                    out.push('\n');
                }
                DesignBlock::MermaidBlock(content) => {
                    let _ = writeln!(out, "```mermaid");
                    let _ = writeln!(out, "{content}");
                    let _ = writeln!(out, "```");
                    out.push('\n');
                }
                DesignBlock::ValidatesLine { raw, refs, markers } => {
                    let resolved = resolved_validates.get(&(section_idx, block_idx));

                    if let Some(resolved_refs) = resolved.filter(|r| !r.is_empty()) {
                        let refs_str = resolved_refs.join(", ");
                        let _ = writeln!(out, "<Validates refs=\"{refs_str}\" />");
                        out.push('\n');
                    } else if refs.is_empty() && markers.is_empty() {
                        // No refs at all — preserve raw line as prose with marker
                        out.push_str(raw);
                        out.push_str("\n\n");
                    }

                    // Emit any parse-time ambiguity markers from this validates line
                    for marker in markers {
                        let _ = writeln!(out, "{marker}");
                        ambiguity_count += 1;
                    }

                    // Emit resolution-phase ambiguity markers if refs couldn't resolve
                    if resolved.is_none() && !refs.is_empty() {
                        let marker = format!(
                            "<!-- TODO(supersigil-import): Could not resolve Validates \
                             references in '{raw}' -->"
                        );
                        let _ = writeln!(out, "{marker}");
                        ambiguity_count += 1;
                    }
                }
            }
        }
    }

    // Append any pre-computed ambiguity markers from the resolution phase
    for marker in ambiguity_markers {
        let _ = writeln!(out, "{marker}");
    }

    (out, ambiguity_count)
}
