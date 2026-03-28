//! Code action provider for duplicate document and component IDs.

use std::collections::HashSet;

use lsp_types::{CodeAction, Diagnostic, Position, Range, TextEdit};

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::diagnostics::{ActionContext, DiagnosticData, DiagnosticSource, GraphDiagnosticKind};
use crate::position::utf16_col;
use crate::supersigil_fence_regions;

// ---------------------------------------------------------------------------
// DuplicateIdProvider
// ---------------------------------------------------------------------------

/// Offers to rename a duplicate document or component ID by appending a `-2`
/// numeric suffix.
#[derive(Debug)]
pub struct DuplicateIdProvider;

impl CodeActionProvider for DuplicateIdProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Graph(
                GraphDiagnosticKind::DuplicateDocumentId
                    | GraphDiagnosticKind::DuplicateComponentId,
            )
        )
    }

    fn actions(
        &self,
        diagnostic: &Diagnostic,
        data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction> {
        let ActionContext::DuplicateId { id, .. } = &data.context else {
            return vec![];
        };

        let is_document_id = matches!(
            data.source,
            DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateDocumentId)
        );

        let existing_ids = if is_document_id {
            collect_document_ids(ctx)
        } else {
            collect_component_ids(ctx.file_content)
        };

        let new_id = find_unique_suffix(id, &existing_ids);

        let candidate = if is_document_id {
            find_frontmatter_id(ctx.file_content, id)
        } else {
            find_component_id(ctx.file_content, id, &diagnostic.range)
        };

        let Some(edit_range) = candidate else {
            return vec![];
        };

        let edit = ctx.single_file_edit(vec![TextEdit {
            range: edit_range,
            new_text: new_id.clone(),
        }]);

        vec![CodeAction {
            title: format!("Rename to '{new_id}'"),
            edit: Some(edit),
            ..Default::default()
        }]
    }
}

/// Try `{base}-2`, `{base}-3`, etc. until finding one not in `existing_ids`.
fn find_unique_suffix(base_id: &str, existing_ids: &HashSet<&str>) -> String {
    for n in 2.. {
        let candidate = format!("{base_id}-{n}");
        if !existing_ids.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!()
}

/// Collect all document IDs from parsed files.
fn collect_document_ids<'a>(ctx: &'a ActionRequestContext) -> HashSet<&'a str> {
    ctx.file_parses
        .values()
        .map(|doc| doc.frontmatter.id.as_str())
        .collect()
}

/// Collect all component `id="..."` values from supersigil-xml fences in the file.
fn collect_component_ids(content: &str) -> HashSet<&str> {
    let regions = supersigil_fence_regions(content);
    let mut ids = HashSet::new();
    for region in &regions {
        for line in content
            .lines()
            .skip(region.open_line + 1)
            .take(region.close_line.saturating_sub(region.open_line + 1))
        {
            let mut search_from = 0;
            while let Some(attr_pos) = line[search_from..].find("id=\"") {
                let abs_pos = search_from + attr_pos;
                let value_start = abs_pos + 4;
                let Some(quote_end) = line[value_start..].find('"') else {
                    break;
                };
                let value = &line[value_start..value_start + quote_end];
                ids.insert(value);
                search_from = value_start + quote_end + 1;
            }
        }
    }
    ids
}

/// Find the range of the ID value in YAML frontmatter (`id: <value>`).
///
/// Searches only within the first frontmatter block (between `---` markers,
/// within the first ~20 lines).
fn find_frontmatter_id(content: &str, id: &str) -> Option<Range> {
    let mut in_frontmatter = false;
    for (line_idx, line_text) in content.lines().enumerate().take(20) {
        if line_text.trim() == "---" {
            if in_frontmatter {
                // End of frontmatter, stop searching.
                break;
            }
            in_frontmatter = true;
            continue;
        }
        if !in_frontmatter {
            continue;
        }

        // Match `id: value` or `id: "value"` or `id: 'value'`.
        let trimmed = line_text.trim_start();
        if !trimmed.starts_with("id:") {
            continue;
        }

        // Find the value portion after `id:`.
        let after_key = &trimmed[3..];
        let value_part = after_key.trim_start();

        // Strip optional quotes.
        let (unquoted, quote_offset) =
            if value_part.starts_with('"') || value_part.starts_with('\'') {
                (&value_part[1..value_part.len().saturating_sub(1)], 1)
            } else {
                (value_part, 0)
            };

        if unquoted.trim_end() != id {
            continue;
        }

        // Compute byte offset of the value within the line.
        let leading_spaces = line_text.len() - trimmed.len();
        // `id:` = 3 chars, then whitespace, then optional quote.
        let value_offset_in_line =
            leading_spaces + 3 + (after_key.len() - value_part.len()) + quote_offset;

        #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
        let line = line_idx as u32;
        let start_col = utf16_col(line_text, value_offset_in_line);
        let end_col = utf16_col(line_text, value_offset_in_line + id.len());

        return Some(Range::new(
            Position::new(line, start_col),
            Position::new(line, end_col),
        ));
    }
    None
}

/// Find the range of the ID value in a component attribute (`id="<value>"`).
///
/// Searches near the diagnostic position for the pattern `id="exact_id"`.
/// Starts at the diagnostic line and searches forward a few lines, then falls
/// back to searching a couple of lines before the diagnostic.
fn find_component_id(content: &str, id: &str, diag_range: &Range) -> Option<Range> {
    let diag_line = diag_range.start.line as usize;
    let needle = format!("id=\"{id}\"");

    // First search from the diagnostic line forward.
    if let Some(found) = search_for_id_needle(content, &needle, id, diag_line, 5) {
        return Some(found);
    }
    // Fall back to a couple of lines before.
    let start = diag_line.saturating_sub(2);
    search_for_id_needle(content, &needle, id, start, diag_line.saturating_sub(start))
}

/// Scan `count` lines starting at `start_line` for `needle`, returning the
/// range of the ID value within the match.
fn search_for_id_needle(
    content: &str,
    needle: &str,
    id: &str,
    start_line: usize,
    count: usize,
) -> Option<Range> {
    for (offset, line_text) in content.lines().enumerate().skip(start_line).take(count) {
        let Some(byte_pos) = line_text.find(needle) else {
            continue;
        };

        // The value starts after `id="` (4 bytes).
        let value_start = byte_pos + 4;
        let value_end = value_start + id.len();

        #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
        let line = offset as u32;
        let start_col = utf16_col(line_text, value_start);
        let end_col = utf16_col(line_text, value_end);

        return Some(Range::new(
            Position::new(line, start_col),
            Position::new(line, end_col),
        ));
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use lsp_types::{Diagnostic, Position, Range};
    use supersigil_rust_macros::verifies;

    use crate::code_actions::CodeActionProvider;
    use crate::code_actions::test_helpers::{TestContext, format_actions};
    use crate::diagnostics::{
        ActionContext, DiagnosticData, DiagnosticSource, GraphDiagnosticKind,
    };

    use super::{DuplicateIdProvider, find_component_id, find_frontmatter_id};

    // -- Test helpers -------------------------------------------------------

    fn make_document_id_diagnostic(message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            message: message.into(),
            ..Default::default()
        }
    }

    fn make_component_id_diagnostic(line: u32, start_col: u32, end_col: u32) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(line, start_col), Position::new(line, end_col)),
            message: "duplicate component ID".into(),
            ..Default::default()
        }
    }

    fn make_document_id_data(id: &str, other_path: &str) -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateDocumentId),
            doc_id: Some(id.to_string()),
            context: ActionContext::DuplicateId {
                id: id.to_string(),
                other_path: other_path.to_string(),
            },
        }
    }

    fn make_component_id_data(id: &str) -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateComponentId),
            doc_id: None,
            context: ActionContext::DuplicateId {
                id: id.to_string(),
                other_path: String::new(),
            },
        }
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_duplicate_document_id() {
        let provider = DuplicateIdProvider;
        let data = make_document_id_data("my-doc", "/other.md");
        assert!(provider.handles(&data));
    }

    #[test]
    fn handles_duplicate_component_id() {
        let provider = DuplicateIdProvider;
        let data = make_component_id_data("task-1");
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_non_duplicate_id_diagnostic() {
        let provider = DuplicateIdProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::BrokenRef),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- actions() for document IDs -----------------------------------------

    #[verifies("lsp-code-actions/req#req-4-3")]
    #[test]
    fn rename_duplicate_document_id() {
        let provider = DuplicateIdProvider;
        let content = "---\nid: my-doc\ntitle: Hello\n---\n# Content\n";
        let diag = make_document_id_diagnostic("duplicate document ID `my-doc`");
        let data = make_document_id_data("my-doc", "/other.md");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Rename to 'my-doc-2'
          edit: file:///tmp/project/spec.md
            @1:4-1:10 replace `my-doc-2`
        "#);
    }

    #[test]
    fn rename_duplicate_document_id_quoted() {
        let provider = DuplicateIdProvider;
        let content = "---\nid: \"my-doc\"\ntitle: Hello\n---\n# Content\n";
        let diag = make_document_id_diagnostic("duplicate document ID `my-doc`");
        let data = make_document_id_data("my-doc", "/other.md");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Rename to 'my-doc-2'
          edit: file:///tmp/project/spec.md
            @1:5-1:11 replace `my-doc-2`
        "#);
    }

    // -- actions() for component IDs ----------------------------------------

    #[test]
    fn rename_duplicate_component_id() {
        let provider = DuplicateIdProvider;
        let content =
            "---\nid: doc\n---\n```supersigil-xml\n<Task id=\"task-1\" status=\"draft\" />\n```\n";
        let diag = make_component_id_diagnostic(4, 0, 5);
        let data = make_component_id_data("task-1");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Rename to 'task-1-2'
          edit: file:///tmp/project/spec.md
            @4:10-4:16 replace `task-1-2`
        "#);
    }

    #[test]
    fn rename_duplicate_component_id_nested() {
        let provider = DuplicateIdProvider;
        let content = "---\nid: doc\n---\n```supersigil-xml\n<Task id=\"task-1\" status=\"draft\">\n  <Criterion id=\"crit-1\" />\n  <Criterion id=\"crit-1\" />\n</Task>\n```\n";
        // Diagnostic for second `crit-1` at line 6.
        let diag = make_component_id_diagnostic(6, 2, 12);
        let data = make_component_id_data("crit-1");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Rename to 'crit-1-2'
          edit: file:///tmp/project/spec.md
            @6:17-6:23 replace `crit-1-2`
        "#);
    }

    #[test]
    fn no_action_when_context_is_none() {
        let provider = DuplicateIdProvider;
        let content = "---\nid: my-doc\n---\n";
        let diag = make_document_id_diagnostic("duplicate document ID `my-doc`");
        let data = DiagnosticData {
            source: DiagnosticSource::Graph(GraphDiagnosticKind::DuplicateDocumentId),
            doc_id: None,
            context: ActionContext::None,
        };

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_action_when_id_not_found() {
        let provider = DuplicateIdProvider;
        // Frontmatter has a different ID than what the diagnostic says.
        let content = "---\nid: other-doc\n---\n";
        let diag = make_document_id_diagnostic("duplicate document ID `my-doc`");
        let data = make_document_id_data("my-doc", "/other.md");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    // -- find_frontmatter_id() ----------------------------------------------

    #[test]
    fn find_frontmatter_id_unquoted() {
        let content = "---\nid: my-doc\ntitle: Hello\n---\n";
        let range = find_frontmatter_id(content, "my-doc").unwrap();
        assert_eq!(range.start, Position::new(1, 4));
        assert_eq!(range.end, Position::new(1, 10));
    }

    #[test]
    fn find_frontmatter_id_double_quoted() {
        let content = "---\nid: \"my-doc\"\ntitle: Hello\n---\n";
        let range = find_frontmatter_id(content, "my-doc").unwrap();
        assert_eq!(range.start, Position::new(1, 5));
        assert_eq!(range.end, Position::new(1, 11));
    }

    #[test]
    fn find_frontmatter_id_single_quoted() {
        let content = "---\nid: 'my-doc'\ntitle: Hello\n---\n";
        let range = find_frontmatter_id(content, "my-doc").unwrap();
        assert_eq!(range.start, Position::new(1, 5));
        assert_eq!(range.end, Position::new(1, 11));
    }

    #[test]
    fn find_frontmatter_id_not_found() {
        let content = "---\nid: other\n---\n";
        assert!(find_frontmatter_id(content, "my-doc").is_none());
    }

    #[test]
    fn find_frontmatter_id_no_frontmatter() {
        let content = "# Just a heading\nSome text\n";
        assert!(find_frontmatter_id(content, "my-doc").is_none());
    }

    // -- find_component_id() ------------------------------------------------

    #[test]
    fn find_component_id_basic() {
        let content = "<Task id=\"task-1\" status=\"draft\" />";
        let diag_range = Range::new(Position::new(0, 0), Position::new(0, 5));
        let range = find_component_id(content, "task-1", &diag_range).unwrap();
        assert_eq!(range.start, Position::new(0, 10));
        assert_eq!(range.end, Position::new(0, 16));
    }

    #[test]
    fn find_component_id_not_found() {
        let content = "<Task id=\"other\" />";
        let diag_range = Range::new(Position::new(0, 0), Position::new(0, 5));
        assert!(find_component_id(content, "task-1", &diag_range).is_none());
    }

    // -- find_unique_suffix() -------------------------------------------------

    #[test]
    fn unique_suffix_skips_existing() {
        use super::find_unique_suffix;
        use std::collections::HashSet;

        let existing: HashSet<&str> = ["task-1", "task-1-2", "task-1-3"].into_iter().collect();
        assert_eq!(find_unique_suffix("task-1", &existing), "task-1-4");
    }

    #[test]
    fn unique_suffix_picks_2_when_free() {
        use super::find_unique_suffix;
        use std::collections::HashSet;

        let existing: HashSet<&str> = ["task-1"].into_iter().collect();
        assert_eq!(find_unique_suffix("task-1", &existing), "task-1-2");
    }

    #[test]
    fn rename_component_id_skips_existing_suffix() {
        let provider = DuplicateIdProvider;
        let content = "---\nid: doc\n---\n```supersigil-xml\n<Task id=\"task-1\" status=\"draft\" />\n<Task id=\"task-1\" status=\"draft\" />\n<Task id=\"task-1-2\" status=\"draft\" />\n```\n";
        let diag = make_component_id_diagnostic(5, 0, 5);
        let data = make_component_id_data("task-1");

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Rename to 'task-1-3'
          edit: file:///tmp/project/spec.md
            @5:10-5:16 replace `task-1-3`
        "#);
    }
}
