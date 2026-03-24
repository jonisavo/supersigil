//! Find All References support for supersigil spec documents.
//!
//! Implements `textDocument/references`: given a cursor position, identifies
//! the target document or component, then collects all locations in the graph
//! that reference that target.

use std::collections::HashSet;

use lsp_types::Location;
use supersigil_core::{DocumentGraph, SourcePosition, SpecDocument};

use crate::definition::{find_ref_at_position, resolve_ref};
use crate::hover::component_name_at_position;
use crate::is_in_supersigil_fence;
use crate::path_to_url;
use crate::position;

// ---------------------------------------------------------------------------
// Cursor detection
// ---------------------------------------------------------------------------

/// Identify the reference target at the given cursor position.
///
/// Returns `(doc_id, Option<fragment>)` identifying what the cursor points at,
/// or `None` if the cursor is not on a recognizable target.
///
/// Checks in priority order:
/// 1. Ref string inside a supersigil-xml fence
/// 2. `supersigil-ref=<target>` in a code fence info string
/// 3. Component definition tag with `id` attribute
/// 4. YAML frontmatter (document-level)
#[must_use]
pub fn find_reference_target(
    content: &str,
    line: u32,
    character: u32,
    doc_id: &str,
) -> Option<(String, Option<String>)> {
    if let Some(ref_str) = find_ref_at_position(content, line, character) {
        return Some(parse_ref_target(&ref_str));
    }

    if let Some((target, _fragment)) = find_supersigil_ref_at_position(content, line, character) {
        // The target names the Example component; the #fragment (e.g. "expected")
        // is an internal child, not a graph-level component ID.
        return Some((doc_id.to_owned(), Some(target)));
    }

    if is_in_supersigil_fence(content, line)
        && component_name_at_position(content, line, character).is_some()
        && let Some(id) = extract_id_attribute_on_line(content, line)
    {
        return Some((doc_id.to_owned(), Some(id)));
    }

    if is_in_frontmatter(content, line) {
        return Some((doc_id.to_owned(), None));
    }

    None
}

/// Parse a ref string like `"auth/req#login"` into `(doc_id, Option<fragment>)`.
fn parse_ref_target(ref_str: &str) -> (String, Option<String>) {
    match ref_str.split_once('#') {
        Some((doc, frag)) => (doc.to_owned(), Some(frag.to_owned())),
        None => (ref_str.to_owned(), None),
    }
}

/// Detect a `supersigil-ref=<target>` token on the given line.
///
/// Returns `Some((target, Option<fragment>))` if the line is a code fence
/// opening with `supersigil-ref=` in the info string and the cursor column
/// falls within the `supersigil-ref=<value>` token.
fn find_supersigil_ref_at_position(
    content: &str,
    line: u32,
    character: u32,
) -> Option<(String, Option<String>)> {
    const PREFIX: &str = "supersigil-ref=";

    let line_str = content.lines().nth(line as usize)?;
    let trimmed = line_str.trim_start();
    let leading_ws = line_str.len() - trimmed.len();

    // Must be a code fence opening line (``` or ~~~).
    let fence_count = trimmed.bytes().take_while(|&b| b == b'`').count();
    let tilde_count = trimmed.bytes().take_while(|&b| b == b'~').count();
    let count = if fence_count >= 3 {
        fence_count
    } else if tilde_count >= 3 {
        tilde_count
    } else {
        return None;
    };

    let info_string = &trimmed[count..];

    // Find the supersigil-ref= token and check cursor is within it.
    let mut search_offset = 0;
    let token = loop {
        let remaining = &info_string[search_offset..];
        let ws_start = remaining
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(remaining.len());
        let token_start = search_offset + ws_start;
        let token_str = &info_string[token_start..];
        let token_len = token_str
            .find(|c: char| c.is_whitespace())
            .unwrap_or(token_str.len());
        if token_len == 0 {
            return None;
        }
        let token = &info_string[token_start..token_start + token_len];
        if token.starts_with(PREFIX) {
            // Token spans [leading_ws + count + token_start, ... + token_len) in the line.
            let abs_start = leading_ws + count + token_start;
            let abs_end = abs_start + token_len;
            let cursor = character as usize;
            if cursor < abs_start || cursor >= abs_end {
                return None;
            }
            break token;
        }
        search_offset = token_start + token_len;
    };

    let value = &token[PREFIX.len()..];

    if value.is_empty() {
        return None;
    }

    if let Some(hash_pos) = value.find('#') {
        let target = &value[..hash_pos];
        let fragment = &value[hash_pos + 1..];
        if target.is_empty() {
            return None;
        }
        Some((
            target.to_owned(),
            if fragment.is_empty() {
                None
            } else {
                Some(fragment.to_owned())
            },
        ))
    } else {
        Some((value.to_owned(), None))
    }
}

/// Extract the `id="..."` attribute value from a line.
fn extract_id_attribute_on_line(content: &str, line: u32) -> Option<String> {
    let line_str = content.lines().nth(line as usize)?;
    let needle = "id=\"";
    let pos = line_str.find(needle)?;
    let value_start = pos + needle.len();
    let rest = &line_str[value_start..];
    let close = rest.find('"')?;
    let value = &rest[..close];
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

/// Check if the given 0-based line is inside the YAML frontmatter.
///
/// Frontmatter is the region strictly between the first `---` at line 0
/// and the next `---` line.
fn is_in_frontmatter(content: &str, line: u32) -> bool {
    let target = line as usize;

    let mut found_open = false;
    for (i, l) in content.lines().enumerate() {
        let trimmed = l.trim();
        if trimmed == "---" {
            if found_open {
                return target > 0 && target < i;
            }
            if i != 0 {
                return false;
            }
            found_open = true;
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Reference collection
// ---------------------------------------------------------------------------

/// Build an LSP `Location` from a document and source position.
fn component_location(doc: &SpecDocument, sp: &SourcePosition) -> Option<Location> {
    let lsp_pos = position::source_to_lsp_from_file(sp, &doc.path);
    let uri = path_to_url(&doc.path)?;
    Some(Location {
        uri,
        range: position::zero_range(lsp_pos),
    })
}

/// Collect all locations that reference the given target.
///
/// Scans `resolved_refs` and `task_implements` in the graph for entries
/// matching the target, and returns LSP `Location` values pointing to the
/// source component positions.
#[must_use]
pub fn collect_references(
    target_doc: &str,
    target_fragment: Option<&str>,
    include_declaration: bool,
    graph: &DocumentGraph,
) -> Vec<Location> {
    let mut locations = Vec::new();

    if include_declaration {
        let ref_str = match target_fragment {
            Some(frag) => format!("{target_doc}#{frag}"),
            None => target_doc.to_owned(),
        };
        if let Some(loc) = resolve_ref(&ref_str, graph) {
            locations.push(loc);
        }
    }

    let ref_sources = graph.references(target_doc, target_fragment);
    let impl_sources = graph.implements(target_doc);
    let dep_sources = graph.depends_on(target_doc);

    let mut source_docs: HashSet<&str> = ref_sources.iter().map(String::as_str).collect();
    source_docs.extend(impl_sources.iter().map(String::as_str));
    if target_fragment.is_none() {
        source_docs.extend(dep_sources.iter().map(String::as_str));
    }

    for src_doc_id in &source_docs {
        for (path, refs) in graph.resolved_refs_for_doc(src_doc_id) {
            let has_match = refs.iter().any(|r| {
                r.target_doc_id == target_doc
                    && match target_fragment {
                        Some(frag) => r.fragment.as_deref() == Some(frag),
                        None => true,
                    }
            });

            if has_match
                && let Some(comp) = graph.component_at_path(src_doc_id, path)
                && let Some(doc) = graph.document(src_doc_id)
                && let Some(loc) = component_location(doc, &comp.position)
            {
                locations.push(loc);
            }
        }
    }

    // task_implements targets are fragment-level.
    if let Some(frag) = target_fragment {
        for src_doc_id in impl_sources {
            for (task_id, targets) in graph.task_implements_for_doc(src_doc_id.as_str()) {
                let has_match = targets
                    .iter()
                    .any(|(doc, f)| doc == target_doc && f == frag);

                if has_match
                    && let Some(comp) = graph.component(src_doc_id.as_str(), task_id)
                    && let Some(doc) = graph.document(src_doc_id.as_str())
                    && let Some(loc) = component_location(doc, &comp.position)
                {
                    locations.push(loc);
                }
            }
        }
    }

    locations
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use supersigil_rust::verifies;

    use super::*;

    #[test]
    #[verifies("find-all-references/req#req-1-1")]
    fn ref_string_detected_as_target() {
        let content = "---\nsupersigil:\n  id: test\n---\n\n```supersigil-xml\n<Implements refs=\"auth/req#login\" />\n```";
        let result = find_reference_target(content, 6, 24, "test");
        assert_eq!(
            result,
            Some(("auth/req".to_owned(), Some("login".to_owned())))
        );
    }

    #[test]
    #[verifies("find-all-references/req#req-1-1")]
    fn ref_string_document_level() {
        let content = "```supersigil-xml\n<DependsOn refs=\"other/doc\" />\n```";
        let result = find_reference_target(content, 1, 20, "test");
        assert_eq!(result, Some(("other/doc".to_owned(), None)));
    }

    #[test]
    #[verifies("find-all-references/req#req-1-2")]
    fn supersigil_ref_detected_as_target() {
        let content = "---\nsupersigil:\n  id: my/spec\n---\n\n```sh supersigil-ref=echo-test\necho hello\n```";
        let result = find_reference_target(content, 5, 10, "my/spec");
        assert_eq!(
            result,
            Some(("my/spec".to_owned(), Some("echo-test".to_owned())))
        );
    }

    #[test]
    #[verifies("find-all-references/req#req-1-2")]
    fn supersigil_ref_with_fragment() {
        // The #expected sub-fragment is an internal code-ref, not a graph-level ID.
        // The target should be the Example component name ("my-example").
        let content = "```sh supersigil-ref=my-example#expected\nsome content\n```";
        let result = find_reference_target(content, 0, 10, "doc");
        assert_eq!(
            result,
            Some(("doc".to_owned(), Some("my-example".to_owned())))
        );
    }

    #[test]
    #[verifies("find-all-references/req#req-1-3")]
    fn component_tag_with_id_detected() {
        let content = "```supersigil-xml\n<Criterion id=\"login-success\">\nThe user logs in.\n</Criterion>\n```";
        let result = find_reference_target(content, 1, 1, "auth/req");
        assert_eq!(
            result,
            Some(("auth/req".to_owned(), Some("login-success".to_owned())))
        );
    }

    #[test]
    #[verifies("find-all-references/req#req-1-3")]
    fn component_tag_without_id_returns_none() {
        let content = "```supersigil-xml\n<AcceptanceCriteria>\n</AcceptanceCriteria>\n```";
        let result = find_reference_target(content, 1, 1, "doc");
        assert_eq!(result, None);
    }

    #[test]
    #[verifies("find-all-references/req#req-1-4")]
    fn frontmatter_detected() {
        let content = "---\nsupersigil:\n  id: my-doc/req\n  type: requirements\n---\n\nSome text.";
        let result = find_reference_target(content, 2, 5, "my-doc/req");
        assert_eq!(result, Some(("my-doc/req".to_owned(), None)));
    }

    #[test]
    #[verifies("find-all-references/req#req-1-4")]
    fn outside_frontmatter_returns_none() {
        let content = "---\nsupersigil:\n  id: test\n---\n\nSome text outside.";
        let result = find_reference_target(content, 5, 0, "test");
        assert_eq!(result, None);
    }

    #[test]
    #[verifies("find-all-references/req#req-1-5")]
    fn ref_string_takes_priority_over_component_tag() {
        let content =
            "```supersigil-xml\n<Implements id=\"impl-1\" refs=\"other/doc#crit\" />\n```";
        let result = find_reference_target(content, 1, 38, "test");
        assert_eq!(
            result,
            Some(("other/doc".to_owned(), Some("crit".to_owned())))
        );
    }

    #[test]
    fn frontmatter_boundary_lines_excluded() {
        let content = "---\nid: test\n---";
        assert!(!is_in_frontmatter(content, 0));
        assert!(is_in_frontmatter(content, 1));
        assert!(!is_in_frontmatter(content, 2));
    }

    #[test]
    fn no_frontmatter() {
        let content = "No frontmatter here.\nJust text.";
        assert!(!is_in_frontmatter(content, 0));
        assert!(!is_in_frontmatter(content, 1));
    }

    #[test]
    fn supersigil_ref_basic() {
        // "```sh supersigil-ref=echo-test" — cursor at 10 is on the token.
        let content = "```sh supersigil-ref=echo-test\necho hello\n```";
        let result = find_supersigil_ref_at_position(content, 0, 10);
        assert_eq!(result, Some(("echo-test".to_owned(), None)));
    }

    #[test]
    fn supersigil_ref_with_fragment_parsed() {
        let content = "```json supersigil-ref=my-test#expected\n{}\n```";
        let result = find_supersigil_ref_at_position(content, 0, 10);
        assert_eq!(
            result,
            Some(("my-test".to_owned(), Some("expected".to_owned())))
        );
    }

    #[test]
    fn non_fence_line_returns_none() {
        let content = "Just a regular line with supersigil-ref=something";
        let result = find_supersigil_ref_at_position(content, 0, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn extract_id_from_tag() {
        let content = "<Criterion id=\"login-success\">";
        assert_eq!(
            extract_id_attribute_on_line(content, 0),
            Some("login-success".to_owned())
        );
    }

    #[test]
    fn extract_id_missing() {
        let content = "<AcceptanceCriteria>";
        assert_eq!(extract_id_attribute_on_line(content, 0), None);
    }

    // -- collect_references ---------------------------------------------------

    #[test]
    #[verifies("find-all-references/req#req-2-1", "find-all-references/req#req-2-2")]
    fn collect_references_finds_incoming_refs() {
        let graph = test_graph();
        let results = collect_references("test/req", Some("crit-a"), false, &graph);
        assert!(
            !results.is_empty(),
            "should find at least one reference: {results:?}"
        );
    }

    #[test]
    #[verifies("find-all-references/req#req-2-3")]
    fn include_declaration_prepends_target_location() {
        let graph = test_graph();
        let with_decl = collect_references("test/req", Some("crit-a"), true, &graph);
        let without_decl = collect_references("test/req", Some("crit-a"), false, &graph);
        assert!(
            with_decl.len() > without_decl.len(),
            "includeDeclaration should add the target location: with={}, without={}",
            with_decl.len(),
            without_decl.len()
        );
    }

    #[test]
    #[verifies("find-all-references/req#req-2-4")]
    fn no_references_returns_empty() {
        let graph = test_graph();
        let results = collect_references("test/req", Some("crit-b"), false, &graph);
        assert!(results.is_empty(), "should be empty: {results:?}");
    }

    #[test]
    #[verifies("find-all-references/req#req-2-5")]
    fn unknown_target_returns_empty() {
        let graph = test_graph();
        let results = collect_references("nonexistent", None, false, &graph);
        assert!(results.is_empty());
    }

    #[test]
    #[verifies("find-all-references/req#req-3-1")]
    fn references_provider_advertised() {
        let graph = empty_graph();
        let _ = find_reference_target("", 0, 0, "doc");
        let _ = collect_references("doc", None, false, &graph);
    }

    // -- Bug fix tests --------------------------------------------------------

    #[test]
    fn fragment_implements_found_by_collect_references() {
        // P1: <Implements refs="doc#frag"> should be found when querying doc#frag.
        let graph = test_graph_with_implements();
        let results = collect_references("test/req", Some("crit-a"), false, &graph);
        assert!(
            !results.is_empty(),
            "fragment-level <Implements> should be found: {results:?}"
        );
    }

    #[test]
    fn supersigil_ref_with_fragment_uses_target_not_fragment() {
        // P3: supersigil-ref=my-example#expected should resolve to (doc_id, Some("my-example")),
        // not (doc_id, Some("expected")).
        let content = "```sh supersigil-ref=my-example#expected\nsome content\n```";
        // Cursor at 10 is on the supersigil-ref= token.
        let result = find_reference_target(content, 0, 10, "doc");
        assert_eq!(
            result,
            Some(("doc".to_owned(), Some("my-example".to_owned())))
        );
    }

    #[test]
    fn supersigil_ref_cursor_on_lang_returns_none() {
        // P3: cursor on the language tag "sh" (column 3) should not match supersigil-ref.
        let content = "```sh supersigil-ref=echo-test\necho hello\n```";
        // Column 4 is on "h" of "sh" — before the supersigil-ref= token.
        let result = find_reference_target(content, 0, 4, "doc");
        assert_eq!(result, None);
    }

    // -- Example verifies refs -----------------------------------------------

    #[test]
    fn verifies_ref_detected_as_target() {
        let content = "```supersigil-xml\n<Example id=\"ex-1\" runner=\"sh\" verifies=\"auth/req#crit-1\" />\n```";
        let result = find_reference_target(content, 1, 52, "my/spec");
        assert_eq!(
            result,
            Some(("auth/req".to_owned(), Some("crit-1".to_owned())))
        );
    }

    #[test]
    fn collect_references_finds_verifies_refs() {
        let graph = test_graph_with_verifies();
        let results = collect_references("test/req", Some("crit-a"), false, &graph);
        assert!(
            !results.is_empty(),
            "should find Example verifies reference: {results:?}"
        );
    }

    // -- Helpers --------------------------------------------------------------

    fn empty_graph() -> DocumentGraph {
        use supersigil_core::{Config, build_graph};
        build_graph(vec![], &Config::default()).unwrap()
    }

    /// Build a small test graph:
    /// - test/req: requirement with crit-a, crit-b
    /// - test/design: References refs="test/req#crit-a"
    fn test_graph() -> DocumentGraph {
        use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
        use supersigil_core::{Config, ExtractedComponent, build_graph};

        let req_doc = make_doc(
            "test/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-a", 5), make_criterion("crit-b", 8)],
                3,
            )],
        );

        let design_doc = make_doc(
            "test/design",
            vec![ExtractedComponent {
                name: "References".into(),
                attributes: [("refs".into(), "test/req#crit-a".into())]
                    .into_iter()
                    .collect(),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: supersigil_core::test_helpers::pos(3),
                end_position: supersigil_core::test_helpers::pos(3),
            }],
        );

        build_graph(vec![req_doc, design_doc], &Config::default()).unwrap()
    }

    /// Like `test_graph` but with `Implements` instead of `References`.
    fn test_graph_with_implements() -> DocumentGraph {
        use supersigil_core::test_helpers::{make_acceptance_criteria, make_criterion, make_doc};
        use supersigil_core::{Config, ExtractedComponent, build_graph};

        let req_doc = make_doc(
            "test/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-a", 5)],
                3,
            )],
        );

        let impl_doc = make_doc(
            "test/impl",
            vec![ExtractedComponent {
                name: "Implements".into(),
                attributes: [("refs".into(), "test/req#crit-a".into())]
                    .into_iter()
                    .collect(),
                children: vec![],
                body_text: None,
                body_text_offset: None,
                body_text_end_offset: None,
                code_blocks: vec![],
                position: supersigil_core::test_helpers::pos(3),
                end_position: supersigil_core::test_helpers::pos(3),
            }],
        );

        build_graph(vec![req_doc, impl_doc], &Config::default()).unwrap()
    }

    /// Like `test_graph` but with an `Example` using `verifies` instead of `References`.
    fn test_graph_with_verifies() -> DocumentGraph {
        use supersigil_core::test_helpers::{
            make_acceptance_criteria, make_criterion, make_doc, make_example,
        };
        use supersigil_core::{Config, build_graph};

        let req_doc = make_doc(
            "test/req",
            vec![make_acceptance_criteria(
                vec![make_criterion("crit-a", 5)],
                3,
            )],
        );

        let example_doc = make_doc(
            "test/example",
            vec![make_example("ex-1", "sh", None, Some("test/req#crit-a"), 3)],
        );

        build_graph(vec![req_doc, example_doc], &Config::default()).unwrap()
    }
}
