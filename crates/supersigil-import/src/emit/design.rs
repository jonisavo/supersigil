use std::fmt::Write;

use crate::emit::emit_front_matter;
use crate::parse::design::{DesignBlock, ParsedDesign};
use crate::refs::{self, RequirementIndex};

/// Emit a design spec document from parsed Kiro design.
///
/// When `req_index` is provided, Validates lines are resolved inline against
/// the requirement index. When absent, an ambiguity marker is emitted instead
/// of the `<Implements>` component.
///
/// Returns `(md_content, ambiguity_count, validates_resolved)`.
#[must_use]
pub fn emit_design_md(
    parsed: &ParsedDesign,
    doc_id: &str,
    req_index: Option<&RequirementIndex<'_>>,
    req_doc_id: &str,
    feature_title: &str,
) -> (String, usize, usize) {
    let mut out = String::new();
    let mut ambiguity_count = 0;
    let mut validates_resolved = 0;

    emit_front_matter(&mut out, doc_id, "design", feature_title);

    // <Implements> or ambiguity marker
    if req_index.is_some() {
        let _ = writeln!(out, "```supersigil-xml");
        let _ = writeln!(out, "<Implements refs=\"{req_doc_id}\" />");
        let _ = writeln!(out, "```");
        out.push('\n');
    } else {
        let marker = "<!-- TODO(supersigil-import): No requirements document found for this \
                       feature; cannot emit <Implements> component -->";
        let _ = writeln!(out, "{marker}");
        out.push('\n');
        ambiguity_count += 1;
    }

    // Emit sections
    for section in &parsed.sections {
        // Section heading (skip for synthetic preamble sections at level 0)
        if section.level > 0 {
            let hashes = "#".repeat(section.level as usize);
            let _ = writeln!(out, "{hashes} {}", section.heading);
            out.push('\n');
        }

        for block in &section.content {
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
                DesignBlock::ValidatesLine { raw, refs, markers } => {
                    // Resolve refs inline against requirement index
                    let resolved = if let Some(index) = req_index
                        && !refs.is_empty()
                    {
                        let (resolved, res_markers) = refs::resolve_refs(refs, index, req_doc_id);
                        validates_resolved += resolved.len();
                        for marker in res_markers {
                            let _ = writeln!(out, "{marker}");
                            ambiguity_count += 1;
                        }
                        resolved
                    } else {
                        Vec::new()
                    };

                    if !resolved.is_empty() {
                        let refs_str = resolved.join(", ");
                        let _ = writeln!(out, "```supersigil-xml");
                        let _ = writeln!(out, "<References refs=\"{refs_str}\" />");
                        let _ = writeln!(out, "```");
                        out.push('\n');
                    } else if refs.is_empty() && markers.is_empty() {
                        // No refs at all — preserve raw line as prose
                        out.push_str(raw);
                        out.push_str("\n\n");
                    }

                    // Emit parse-time ambiguity markers from this validates line
                    for marker in markers {
                        let _ = writeln!(out, "{marker}");
                        ambiguity_count += 1;
                    }

                    // Emit resolution-phase marker if refs couldn't resolve
                    if resolved.is_empty() && !refs.is_empty() {
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

    (out, ambiguity_count, validates_resolved)
}
