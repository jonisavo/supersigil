//! Code content resolution: link `supersigil-ref` fences to extracted components.
//!
//! After component extraction (Task 3) produces `ExtractedComponent` values and
//! Markdown fence extraction (Task 1) produces `RefFence` values, this module
//! resolves each `RefFence` to its target `Example` or `Expected` component and
//! populates the component's `code_blocks` field.
//!
//! **Resolution rules:**
//! - `RefFence` with no fragment → targets an `Example` component by `id` attribute.
//! - `RefFence` with `fragment == "expected"` → targets the `Expected` child of
//!   an `Example` matched by `id` attribute.
//! - Inline `body_text` falls back to a `CodeBlock` when no `RefFence` targets
//!   the component.
//!
//! **Error conditions:**
//! - Dual-source: both inline text and a linked `RefFence` exist.
//! - Orphan ref: a `RefFence` targets no component.
//! - Multiple refs: multiple `RefFence` values target the same component.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::Path;

use supersigil_core::{
    CodeBlock, EXAMPLE, EXPECTED, EXPECTED_FRAGMENT, ExtractedComponent, ParseError, SpanKind,
};

use crate::markdown_fences::RefFence;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Resolve `supersigil-ref` fences against extracted components.
///
/// For each `RefFence`, finds the target `Example` or `Expected` component
/// and attaches the fence content as a `CodeBlock`. Components with
/// `body_text` but no linked ref fence get their body text converted to a
/// code block as a fallback.
///
/// Errors are appended to `errors` for: orphan refs, duplicate refs
/// targeting the same component, and dual-source conflicts (both inline
/// text and a ref fence).
pub fn resolve_code_refs(
    components: &mut [ExtractedComponent],
    ref_fences: &[RefFence],
    path: &Path,
    errors: &mut Vec<ParseError>,
) {
    // Track which (example_id, fragment) pairs have been targeted by a ref fence.
    // Value is the index into ref_fences for error reporting.
    let mut targeted: HashMap<(String, Option<String>), usize> = HashMap::new();

    for (idx, rf) in ref_fences.iter().enumerate() {
        let key = (rf.target.clone(), rf.fragment.clone());

        match targeted.entry(key) {
            Entry::Occupied(_) => {
                errors.push(ParseError::DuplicateCodeRef {
                    path: path.to_path_buf(),
                    target: format_ref_target(&rf.target, rf.fragment.as_deref()),
                });
                continue;
            }
            Entry::Vacant(entry) => {
                entry.insert(idx);
            }
        }

        // Find the target component.
        let found = find_target(components, &rf.target, rf.fragment.as_deref());

        match found {
            None => {
                errors.push(ParseError::OrphanCodeRef {
                    path: path.to_path_buf(),
                    target: format_ref_target(&rf.target, rf.fragment.as_deref()),
                    content_offset: rf.content_offset,
                });
            }
            Some(target) => {
                // Check for dual-source conflict.
                if target.body_text.is_some() {
                    errors.push(ParseError::DualSourceConflict {
                        path: path.to_path_buf(),
                        target: format_ref_target(&rf.target, rf.fragment.as_deref()),
                        content_offset: rf.content_offset,
                    });
                    continue;
                }

                target.code_blocks.push(CodeBlock {
                    lang: rf.lang.clone(),
                    content: rf.content.clone(),
                    content_offset: rf.content_offset,
                    content_end_offset: rf.content_offset + rf.content.len(),
                    span_kind: SpanKind::RefFence,
                });
            }
        }
    }

    // Inline fallback pass.
    apply_inline_fallback(components);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find the target component for a ref fence.
///
/// - `fragment == None` → find an `Example` whose `id` attribute matches `target`.
/// - `fragment == Some("expected")` → find the `Expected` child of that `Example`.
fn find_target<'a>(
    components: &'a mut [ExtractedComponent],
    target: &str,
    fragment: Option<&str>,
) -> Option<&'a mut ExtractedComponent> {
    for comp in components.iter_mut() {
        if comp.name == EXAMPLE && comp.attributes.get("id").map(String::as_str) == Some(target) {
            match fragment {
                None => return Some(comp),
                Some(EXPECTED_FRAGMENT) => {
                    // Find the Expected child.
                    for child in &mut comp.children {
                        if child.name == EXPECTED {
                            return Some(child);
                        }
                    }
                    // No Expected child found — treated as not found.
                    return None;
                }
                Some(_) => {
                    // Unknown fragment — not found.
                    return None;
                }
            }
        }
        // Recurse into children to find nested Examples.
        if let Some(found) = find_target(&mut comp.children, target, fragment) {
            return Some(found);
        }
    }
    None
}

/// Apply inline text fallback for `Example` and `Expected` components.
///
/// If a component has `body_text` set and `code_blocks` is empty, the body
/// text is consumed into a `CodeBlock` with `lang: None`.
fn apply_inline_fallback(components: &mut [ExtractedComponent]) {
    for comp in components.iter_mut() {
        if (comp.name == EXAMPLE || comp.name == EXPECTED)
            && comp.code_blocks.is_empty()
            && comp.body_text.is_some()
        {
            let text = comp.body_text.take().unwrap();
            let start = comp.body_text_offset.unwrap_or(comp.position.byte_offset);
            let end = comp.body_text_end_offset.unwrap_or(start + text.len());
            comp.code_blocks.push(CodeBlock {
                lang: None,
                content: text,
                content_offset: start,
                content_end_offset: end,
                span_kind: SpanKind::XmlInline,
            });
        }
        // Recurse into children.
        apply_inline_fallback(&mut comp.children);
    }
}

/// Format a ref target for error messages (e.g. `"my-test"` or `"my-test#expected"`).
fn format_ref_target(target: &str, fragment: Option<&str>) -> String {
    match fragment {
        Some(f) => format!("{target}#{f}"),
        None => target.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use supersigil_core::SourcePosition;

    use super::*;

    /// Helper: create an `Example` component with the given id and optional body text.
    fn example(id: &str, body_text: Option<&str>) -> ExtractedComponent {
        ExtractedComponent {
            name: "Example".into(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("id".into(), id.into());
                m.insert("runner".into(), "shell".into());
                m
            },
            children: vec![],
            body_text: body_text.map(ToOwned::to_owned),
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            end_position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
        }
    }

    /// Helper: create an `Example` with an `Expected` child.
    fn example_with_expected(
        id: &str,
        example_body: Option<&str>,
        expected_body: Option<&str>,
    ) -> ExtractedComponent {
        let mut ex = example(id, example_body);
        ex.children.push(ExtractedComponent {
            name: "Expected".into(),
            attributes: HashMap::new(),
            children: vec![],
            body_text: expected_body.map(ToOwned::to_owned),
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: SourcePosition {
                byte_offset: 100,
                line: 5,
                column: 3,
            },
            end_position: SourcePosition {
                byte_offset: 100,
                line: 5,
                column: 3,
            },
        });
        ex
    }

    /// Helper: create a `RefFence`.
    fn ref_fence(
        target: &str,
        fragment: Option<&str>,
        lang: Option<&str>,
        content: &str,
        offset: usize,
    ) -> RefFence {
        RefFence {
            target: target.into(),
            fragment: fragment.map(Into::into),
            lang: lang.map(Into::into),
            content: content.into(),
            content_offset: offset,
        }
    }

    fn test_path() -> &'static Path {
        Path::new("test.md")
    }

    // -- Successful resolution ------------------------------------------------

    #[test]
    fn resolves_ref_fence_to_example() {
        let mut components = vec![example("echo-test", None)];
        let fences = vec![ref_fence("echo-test", None, Some("sh"), "echo hello", 200)];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
        assert_eq!(components[0].code_blocks.len(), 1);
        assert_eq!(components[0].code_blocks[0].lang.as_deref(), Some("sh"));
        assert_eq!(components[0].code_blocks[0].content, "echo hello");
        assert_eq!(components[0].code_blocks[0].content_offset, 200);
    }

    // -- Implicit #expected fragment ------------------------------------------

    #[test]
    fn resolves_expected_fragment() {
        let mut components = vec![example_with_expected("create-task", None, None)];
        let fences = vec![ref_fence(
            "create-task",
            Some("expected"),
            Some("json"),
            r#"{"status":"ok"}"#,
            300,
        )];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
        // The Example itself should have no code blocks.
        assert!(components[0].code_blocks.is_empty());
        // The Expected child should have the code block.
        let expected = &components[0].children[0];
        assert_eq!(expected.code_blocks.len(), 1);
        assert_eq!(expected.code_blocks[0].lang.as_deref(), Some("json"));
        assert_eq!(expected.code_blocks[0].content, r#"{"status":"ok"}"#);
        assert_eq!(expected.code_blocks[0].content_offset, 300);
    }

    // -- Both Example and Expected resolved from separate refs ----------------

    #[test]
    fn resolves_both_example_and_expected_from_separate_refs() {
        let mut components = vec![example_with_expected("my-test", None, None)];
        let fences = vec![
            ref_fence("my-test", None, Some("sh"), "run test", 200),
            ref_fence("my-test", Some("expected"), Some("txt"), "pass", 400),
        ];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
        assert_eq!(components[0].code_blocks.len(), 1);
        assert_eq!(components[0].code_blocks[0].content, "run test");

        let expected = &components[0].children[0];
        assert_eq!(expected.code_blocks.len(), 1);
        assert_eq!(expected.code_blocks[0].content, "pass");
    }

    // -- Inline text fallback -------------------------------------------------

    #[test]
    fn inline_text_fallback_for_example() {
        let mut components = vec![example("inline-test", Some("echo inline"))];
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
        // body_text should be consumed.
        assert_eq!(components[0].body_text, None);
        // A code block should be created from the body text.
        assert_eq!(components[0].code_blocks.len(), 1);
        assert_eq!(components[0].code_blocks[0].lang, None);
        assert_eq!(components[0].code_blocks[0].content, "echo inline");
        assert_eq!(
            components[0].code_blocks[0].content_offset,
            components[0].position.byte_offset
        );
    }

    #[test]
    fn inline_text_fallback_for_expected() {
        let mut components = vec![example_with_expected("ft", None, Some("expected output"))];
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
        let expected = &components[0].children[0];
        assert_eq!(expected.body_text, None);
        assert_eq!(expected.code_blocks.len(), 1);
        assert_eq!(expected.code_blocks[0].content, "expected output");
        assert_eq!(expected.code_blocks[0].lang, None);
    }

    #[test]
    fn non_example_component_body_text_is_not_touched() {
        // A Criterion component with body_text should NOT get inline fallback.
        let mut components = vec![ExtractedComponent {
            name: "Criterion".into(),
            attributes: {
                let mut m = HashMap::new();
                m.insert("id".into(), "crit-1".into());
                m
            },
            children: vec![],
            body_text: Some("The system shall...".into()),
            body_text_offset: None,
            body_text_end_offset: None,
            code_blocks: vec![],
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            end_position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
        }];
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty());
        // body_text should remain untouched.
        assert_eq!(
            components[0].body_text.as_deref(),
            Some("The system shall...")
        );
        assert!(components[0].code_blocks.is_empty());
    }

    // -- Dual-source conflict -------------------------------------------------

    #[test]
    fn dual_source_conflict_error() {
        let mut components = vec![example("conflict-test", Some("inline code"))];
        let fences = vec![ref_fence(
            "conflict-test",
            None,
            Some("sh"),
            "ref code",
            200,
        )];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ParseError::DualSourceConflict { target, .. } if target == "conflict-test"),
            "expected DualSourceConflict, got: {:?}",
            errors[0]
        );
        // The ref should NOT be applied (conflict prevents it).
        // body_text is still present (becomes inline fallback after resolution).
        // Actually, since the ref was rejected, inline fallback kicks in.
        assert_eq!(components[0].code_blocks.len(), 1);
        assert_eq!(components[0].code_blocks[0].content, "inline code");
        assert_eq!(components[0].body_text, None);
    }

    #[test]
    fn dual_source_conflict_on_expected_child() {
        let mut components = vec![example_with_expected(
            "ds-test",
            None,
            Some("inline expected"),
        )];
        let fences = vec![ref_fence(
            "ds-test",
            Some("expected"),
            Some("json"),
            "ref expected",
            300,
        )];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ParseError::DualSourceConflict { target, .. } if target == "ds-test#expected"),
        );
    }

    // -- Orphan ref -----------------------------------------------------------

    #[test]
    fn orphan_ref_error() {
        let mut components = vec![example("existing-test", None)];
        let fences = vec![ref_fence("nonexistent-test", None, Some("sh"), "code", 200)];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ParseError::OrphanCodeRef { target, .. } if target == "nonexistent-test"),
            "expected OrphanCodeRef, got: {:?}",
            errors[0]
        );
    }

    #[test]
    fn orphan_ref_for_expected_fragment_without_expected_child() {
        // Example exists but has no Expected child.
        let mut components = vec![example("no-expected", None)];
        let fences = vec![ref_fence(
            "no-expected",
            Some("expected"),
            Some("json"),
            "{}",
            200,
        )];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ParseError::OrphanCodeRef { target, .. } if target == "no-expected#expected"),
        );
    }

    // -- Multiple refs targeting same component (duplicate ref) ---------------

    #[test]
    fn duplicate_ref_error() {
        let mut components = vec![example("dup-test", None)];
        let fences = vec![
            ref_fence("dup-test", None, Some("sh"), "first", 200),
            ref_fence("dup-test", None, Some("sh"), "second", 300),
        ];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ParseError::DuplicateCodeRef { target, .. } if target == "dup-test"),
            "expected DuplicateCodeRef, got: {:?}",
            errors[0]
        );
        // Only the first ref should be applied.
        assert_eq!(components[0].code_blocks.len(), 1);
        assert_eq!(components[0].code_blocks[0].content, "first");
    }

    // -- No ref fences, no body text: nothing happens -------------------------

    #[test]
    fn no_refs_no_body_text_leaves_component_unchanged() {
        let mut components = vec![example("empty-test", None)];
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty());
        assert!(components[0].code_blocks.is_empty());
        assert_eq!(components[0].body_text, None);
    }

    // -- Empty ref_fences with no components ----------------------------------

    #[test]
    fn empty_inputs_produce_no_errors() {
        let mut components: Vec<ExtractedComponent> = vec![];
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty());
    }

    // -- Inline fallback with entity-decoded content ---------------------------

    #[test]
    fn inline_fallback_preserves_raw_end_offset_for_entity_content() {
        // Simulate an Expected component whose body text was decoded from
        // entities: raw source "&lt;html&gt;" (12 bytes) → decoded "<html>" (6 bytes).
        // body_text_offset=100, body_text_end_offset=112 (raw span).
        let mut components = vec![{
            let mut ex = example("entity-test", None);
            ex.children.push(ExtractedComponent {
                name: "Expected".into(),
                attributes: HashMap::new(),
                children: vec![],
                body_text: Some("<html>".into()),
                body_text_offset: Some(100),
                body_text_end_offset: Some(112), // raw span: "&lt;html&gt;" = 12 bytes
                code_blocks: vec![],
                position: SourcePosition {
                    byte_offset: 80,
                    line: 5,
                    column: 3,
                },
                end_position: SourcePosition {
                    byte_offset: 80,
                    line: 5,
                    column: 3,
                },
            });
            ex
        }];
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
        let expected = &components[0].children[0];
        assert_eq!(expected.code_blocks.len(), 1);
        let cb = &expected.code_blocks[0];
        assert_eq!(cb.content, "<html>");
        assert_eq!(cb.content_offset, 100);
        // The end offset should be 112 (raw), NOT 100 + 6 = 106 (decoded)
        assert_eq!(
            cb.content_end_offset, 112,
            "content_end_offset should use raw source end, not decoded length"
        );
    }

    #[test]
    fn inline_fallback_without_end_offset_falls_back_to_decoded_length() {
        // When body_text_end_offset is None (no entities), the fallback
        // should compute end from offset + decoded length.
        let mut components = vec![example("plain-test", Some("echo hello"))];
        components[0].body_text_offset = Some(50);
        // body_text_end_offset remains None
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty());
        let cb = &components[0].code_blocks[0];
        assert_eq!(cb.content, "echo hello");
        assert_eq!(cb.content_offset, 50);
        assert_eq!(
            cb.content_end_offset,
            50 + "echo hello".len(),
            "without body_text_end_offset, should fall back to offset + decoded len"
        );
    }

    #[test]
    fn ref_fence_content_end_offset_equals_offset_plus_content_len() {
        let mut components = vec![example("ref-test", None)];
        let fences = vec![ref_fence("ref-test", None, Some("sh"), "echo hello", 200)];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty());
        let cb = &components[0].code_blocks[0];
        assert_eq!(cb.content_offset, 200);
        assert_eq!(
            cb.content_end_offset,
            200 + "echo hello".len(),
            "ref fence content_end_offset should be offset + content.len()"
        );
    }

    // -- SpanKind propagation -------------------------------------------------

    #[test]
    fn ref_fence_code_block_has_ref_fence_span_kind() {
        let mut components = vec![example("sk-test", None)];
        let fences = vec![ref_fence("sk-test", None, Some("sh"), "echo hi", 200)];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty());
        assert_eq!(components[0].code_blocks[0].span_kind, SpanKind::RefFence,);
    }

    #[test]
    fn inline_fallback_code_block_has_xml_inline_span_kind() {
        let mut components = vec![example("inline-sk", Some("echo inline"))];
        let fences: Vec<RefFence> = vec![];
        let mut errors = Vec::new();

        resolve_code_refs(&mut components, &fences, test_path(), &mut errors);

        assert!(errors.is_empty());
        assert_eq!(components[0].code_blocks[0].span_kind, SpanKind::XmlInline,);
    }
}
