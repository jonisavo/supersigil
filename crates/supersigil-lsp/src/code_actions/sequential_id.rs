//! Code action provider for sequential ID gaps and ordering issues.

use std::collections::HashMap;

use lsp_types::{CodeAction, Diagnostic, Position, Range, TextEdit};
use supersigil_verify::RuleName;

use crate::code_actions::{ActionRequestContext, CodeActionProvider};
use crate::diagnostics::{DiagnosticData, DiagnosticSource};
use crate::position::utf16_col;
use crate::{FenceRegion, supersigil_fence_regions};

// ---------------------------------------------------------------------------
// SequentialIdProvider
// ---------------------------------------------------------------------------

/// Offers to renumber sequential component IDs to close gaps and restore order.
///
/// Handles `SequentialIdGap` and `SequentialIdOrder` findings by scanning the
/// file for all `id="..."` attributes within supersigil-xml fences, grouping
/// them by prefix, and renumbering each group sequentially based on order of
/// appearance.
#[derive(Debug)]
pub struct SequentialIdProvider;

impl CodeActionProvider for SequentialIdProvider {
    fn handles(&self, data: &DiagnosticData) -> bool {
        matches!(
            data.source,
            DiagnosticSource::Verify(RuleName::SequentialIdGap | RuleName::SequentialIdOrder)
        )
    }

    fn actions(
        &self,
        _diagnostic: &Diagnostic,
        _data: &DiagnosticData,
        ctx: &ActionRequestContext,
    ) -> Vec<CodeAction> {
        let regions = supersigil_fence_regions(ctx.file_content);
        let occurrences = find_sequential_ids(ctx.file_content, &regions);
        if occurrences.is_empty() {
            return vec![];
        }

        // Group by prefix, preserving order of appearance.
        let mut groups: Vec<(&str, Vec<&IdOccurrence>)> = Vec::new();
        let mut prefix_index: HashMap<&str, usize> = HashMap::new();
        for occ in &occurrences {
            if let Some(&idx) = prefix_index.get(occ.prefix.as_str()) {
                groups[idx].1.push(occ);
            } else {
                let idx = groups.len();
                prefix_index.insert(&occ.prefix, idx);
                groups.push((&occ.prefix, vec![occ]));
            }
        }

        // Generate edits for groups that need renumbering, and build a rename map.
        let mut edits = Vec::new();
        let mut rename_map: HashMap<String, String> = HashMap::new();
        for (prefix, group) in &groups {
            for (idx, occ) in group.iter().enumerate() {
                let expected_num = idx + 1;
                if occ.number != expected_num {
                    let old_id = format!("{prefix}-{}", occ.number);
                    let new_id = format!("{prefix}-{expected_num}");
                    rename_map.insert(old_id, new_id.clone());
                    edits.push(TextEdit {
                        range: occ.range,
                        new_text: new_id,
                    });
                }
            }
        }

        if edits.is_empty() {
            return vec![];
        }

        // Second pass: rewrite `depends="..."` attribute values that reference
        // renamed IDs.
        let depends_edits = find_depends_edits(ctx.file_content, &regions, &rename_map);
        edits.extend(depends_edits);

        let edit = ctx.single_file_edit(edits);

        vec![CodeAction {
            title: "Renumber sequential IDs".to_string(),
            edit: Some(edit),
            ..Default::default()
        }]
    }
}

// ---------------------------------------------------------------------------
// ID parsing and scanning
// ---------------------------------------------------------------------------

/// A sequential ID occurrence found in the file.
#[derive(Debug)]
struct IdOccurrence {
    /// The prefix portion (e.g., `req-1` for `req-1-3`).
    prefix: String,
    /// The numeric suffix (e.g., `3` for `req-1-3`).
    number: usize,
    /// The LSP range covering the entire ID value (inside `id="..."`).
    range: Range,
}

/// Parse an ID into (prefix, number) by splitting at the last `-`.
///
/// Returns `None` if the ID doesn't end with a numeric segment.
fn parse_sequential_id(id: &str) -> Option<(String, usize)> {
    let last_dash = id.rfind('-')?;
    let prefix = &id[..last_dash];
    let num_str = &id[last_dash + 1..];

    // The numeric part must be non-empty and all ASCII digits.
    if num_str.is_empty() || !num_str.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    // Reject leading zeros (except for "0" itself).
    if num_str.len() > 1 && num_str.starts_with('0') {
        return None;
    }

    let number: usize = num_str.parse().ok()?;
    Some((prefix.to_string(), number))
}

/// Scan file content for `depends="..."` attributes within supersigil-xml fences
/// and generate edits to rewrite any values that appear in the rename map.
fn find_depends_edits(
    content: &str,
    regions: &[FenceRegion],
    rename_map: &HashMap<String, String>,
) -> Vec<TextEdit> {
    if rename_map.is_empty() {
        return vec![];
    }

    let mut edits = Vec::new();
    let needle = "depends=\"";

    for region in regions {
        for (line_idx, line_text) in content
            .lines()
            .enumerate()
            .skip(region.open_line + 1)
            .take(region.close_line.saturating_sub(region.open_line + 1))
        {
            let mut search_from = 0;
            while let Some(attr_pos) = line_text[search_from..].find(needle) {
                let abs_pos = search_from + attr_pos;
                let value_start = abs_pos + needle.len();
                let Some(quote_end) = line_text[value_start..].find('"') else {
                    break;
                };
                let value_end = value_start + quote_end;
                let attr_value = &line_text[value_start..value_end];

                // Split by comma and check each part against the rename map.
                let parts: Vec<&str> = attr_value.split(',').map(str::trim).collect();
                let mut any_renamed = false;
                let new_parts: Vec<String> = parts
                    .iter()
                    .map(|&part| {
                        if let Some(new_name) = rename_map.get(part) {
                            any_renamed = true;
                            new_name.clone()
                        } else {
                            part.to_string()
                        }
                    })
                    .collect();

                if any_renamed {
                    let new_value = new_parts.join(", ");
                    #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
                    let line = line_idx as u32;
                    let start_col = utf16_col(line_text, value_start);
                    let end_col = utf16_col(line_text, value_end);

                    edits.push(TextEdit {
                        range: Range::new(
                            Position::new(line, start_col),
                            Position::new(line, end_col),
                        ),
                        new_text: new_value,
                    });
                }

                search_from = value_end + 1;
            }
        }
    }

    edits
}

/// Scan file content for all `id="..."` attributes within supersigil-xml fences.
///
/// Returns occurrences in order of appearance.
fn find_sequential_ids(content: &str, regions: &[FenceRegion]) -> Vec<IdOccurrence> {
    let mut occurrences = Vec::new();

    for region in regions {
        for (line_idx, line_text) in content
            .lines()
            .enumerate()
            .skip(region.open_line + 1)
            .take(region.close_line.saturating_sub(region.open_line + 1))
        {
            // Search for all id="..." occurrences on this line.
            let mut search_from = 0;
            while let Some(attr_pos) = line_text[search_from..].find("id=\"") {
                let abs_pos = search_from + attr_pos;
                let value_start = abs_pos + 4; // after `id="`
                let Some(quote_end) = line_text[value_start..].find('"') else {
                    break;
                };
                let value_end = value_start + quote_end;
                let id_value = &line_text[value_start..value_end];

                if let Some((prefix, number)) = parse_sequential_id(id_value) {
                    #[allow(clippy::cast_possible_truncation, reason = "line count fits u32")]
                    let line = line_idx as u32;
                    let start_col = utf16_col(line_text, value_start);
                    let end_col = utf16_col(line_text, value_end);

                    occurrences.push(IdOccurrence {
                        prefix,
                        number,
                        range: Range::new(
                            Position::new(line, start_col),
                            Position::new(line, end_col),
                        ),
                    });
                }

                search_from = value_end + 1;
            }
        }
    }

    occurrences
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use lsp_types::{Diagnostic, Position, Range};
    use supersigil_rust_macros::verifies;
    use supersigil_verify::RuleName;

    use crate::code_actions::CodeActionProvider;
    use crate::code_actions::test_helpers::{TestContext, format_actions};
    use crate::diagnostics::{ActionContext, DiagnosticData, DiagnosticSource};

    use crate::supersigil_fence_regions;

    use super::{SequentialIdProvider, find_sequential_ids, parse_sequential_id};

    // -- Test helpers -------------------------------------------------------

    fn make_diagnostic(message: &str) -> Diagnostic {
        Diagnostic {
            range: Range::new(Position::new(0, 0), Position::new(0, 0)),
            message: message.into(),
            ..Default::default()
        }
    }

    fn make_data(rule: RuleName) -> DiagnosticData {
        DiagnosticData {
            source: DiagnosticSource::Verify(rule),
            doc_id: Some("my-doc".into()),
            context: ActionContext::None,
        }
    }

    // -- parse_sequential_id() ----------------------------------------------

    #[test]
    fn parse_simple_id() {
        let (prefix, num) = parse_sequential_id("req-1").unwrap();
        assert_eq!(prefix, "req");
        assert_eq!(num, 1);
    }

    #[test]
    fn parse_multi_part_prefix() {
        let (prefix, num) = parse_sequential_id("req-1-3").unwrap();
        assert_eq!(prefix, "req-1");
        assert_eq!(num, 3);
    }

    #[test]
    fn parse_rejects_no_dash() {
        assert!(parse_sequential_id("req").is_none());
    }

    #[test]
    fn parse_rejects_non_numeric_suffix() {
        assert!(parse_sequential_id("req-abc").is_none());
    }

    #[test]
    fn parse_rejects_leading_zeros() {
        assert!(parse_sequential_id("req-01").is_none());
    }

    #[test]
    fn parse_accepts_zero() {
        let (prefix, num) = parse_sequential_id("task-0").unwrap();
        assert_eq!(prefix, "task");
        assert_eq!(num, 0);
    }

    // -- find_sequential_ids() ----------------------------------------------

    #[test]
    fn finds_ids_in_fence() {
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" />
```
";
        let ids = find_sequential_ids(content, &supersigil_fence_regions(content));
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].prefix, "task");
        assert_eq!(ids[0].number, 1);
        assert_eq!(ids[1].prefix, "task");
        assert_eq!(ids[1].number, 3);
    }

    #[test]
    fn ignores_ids_outside_fence() {
        let content = "\
---
id: my-doc
---
<Task id=\"task-1\" status=\"draft\" />
```supersigil-xml
<Task id=\"task-2\" status=\"draft\" />
```
";
        let ids = find_sequential_ids(content, &supersigil_fence_regions(content));
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0].number, 2);
    }

    #[test]
    fn handles_multiple_fences() {
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
```

Some prose.

```supersigil-xml
<Task id=\"task-3\" status=\"draft\" />
```
";
        let ids = find_sequential_ids(content, &supersigil_fence_regions(content));
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].number, 1);
        assert_eq!(ids[1].number, 3);
    }

    // -- handles() ----------------------------------------------------------

    #[test]
    fn handles_sequential_id_gap() {
        let provider = SequentialIdProvider;
        let data = make_data(RuleName::SequentialIdGap);
        assert!(provider.handles(&data));
    }

    #[test]
    fn handles_sequential_id_order() {
        let provider = SequentialIdProvider;
        let data = make_data(RuleName::SequentialIdOrder);
        assert!(provider.handles(&data));
    }

    #[test]
    fn rejects_other_rules() {
        let provider = SequentialIdProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Verify(RuleName::IncompleteDecision),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    #[test]
    fn rejects_parse_diagnostic() {
        use crate::diagnostics::ParseDiagnosticKind;

        let provider = SequentialIdProvider;
        let data = DiagnosticData {
            source: DiagnosticSource::Parse(ParseDiagnosticKind::XmlSyntaxError),
            doc_id: None,
            context: ActionContext::None,
        };
        assert!(!provider.handles(&data));
    }

    // -- actions() ----------------------------------------------------------

    #[verifies("lsp-code-actions/req#req-4-7")]
    #[test]
    fn renumber_gap_in_sequence() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" />
<Task id=\"task-4\" status=\"draft\" />
```
";
        let diag = make_diagnostic(
            "gap in sequence: `task-2` is missing (between `task-1` and `task-3` in document `my-doc`)",
        );
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @5:10-5:16 replace `task-2`
            @6:10-6:16 replace `task-3`
        "#);
    }

    #[test]
    fn renumber_out_of_order() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-2\" status=\"draft\" />
<Task id=\"task-1\" status=\"draft\" />
```
";
        let diag = make_diagnostic("`task-1` is declared after `task-2` in document `my-doc`");
        let data = make_data(RuleName::SequentialIdOrder);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @4:10-4:16 replace `task-1`
            @5:10-5:16 replace `task-2`
        "#);
    }

    #[test]
    fn renumber_multi_part_prefix() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Criterion id=\"req-1-1\" />
<Criterion id=\"req-1-3\" />
<Criterion id=\"req-1-4\" />
```
";
        let diag = make_diagnostic(
            "gap in sequence: `req-1-2` is missing (between `req-1-1` and `req-1-3` in document `my-doc`)",
        );
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @5:15-5:22 replace `req-1-2`
            @6:15-6:22 replace `req-1-3`
        "#);
    }

    #[test]
    fn no_edits_when_already_sequential() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-2\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" />
```
";
        let diag = make_diagnostic("some sequential id message");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn no_action_on_empty_file() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
Some text without components.
";
        let diag = make_diagnostic("some sequential id message");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        assert!(actions.is_empty());
    }

    #[test]
    fn multiple_prefix_groups() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" />
<Criterion id=\"crit-1\" />
<Criterion id=\"crit-5\" />
```
";
        let diag = make_diagnostic("gap in sequence");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @5:10-5:16 replace `task-2`
            @7:15-7:21 replace `crit-2`
        "#);
    }

    #[test]
    fn nested_components_with_gap() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\">
  <Criterion id=\"crit-1\" />
  <Criterion id=\"crit-3\" />
</Task>
```
";
        let diag = make_diagnostic("gap in sequence: `crit-2` is missing");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @6:17-6:23 replace `crit-2`
        "#);
    }

    #[test]
    fn non_sequential_ids_are_ignored() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Decision id=\"use-postgres\" />
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" />
```
";
        let diag = make_diagnostic("gap in sequence");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @6:10-6:16 replace `task-2`
        "#);
    }

    // -- depends rewriting ---------------------------------------------------

    #[test]
    fn renumber_also_rewrites_depends() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" depends=\"task-1\" />
<Task id=\"task-4\" status=\"draft\" depends=\"task-3\" />
```
";
        let diag = make_diagnostic("gap in sequence: `task-2` is missing");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @5:10-5:16 replace `task-2`
            @6:10-6:16 replace `task-3`
            @6:42-6:48 replace `task-2`
        "#);
    }

    #[test]
    fn renumber_rewrites_depends_comma_separated() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" />
<Task id=\"task-4\" status=\"draft\" depends=\"task-3, task-1\" />
```
";
        let diag = make_diagnostic("gap in sequence");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @5:10-5:16 replace `task-2`
            @6:10-6:16 replace `task-3`
            @6:42-6:56 replace `task-2, task-1`
        "#);
    }

    #[test]
    fn renumber_no_depends_edit_when_not_renamed() {
        let provider = SequentialIdProvider;
        let content = "\
---
id: my-doc
---
```supersigil-xml
<Task id=\"task-1\" status=\"draft\" />
<Task id=\"task-3\" status=\"draft\" depends=\"task-1\" />
```
";
        let diag = make_diagnostic("gap in sequence");
        let data = make_data(RuleName::SequentialIdGap);

        let tc = TestContext::new();
        let ctx = tc.make_ctx(content);

        let actions = provider.actions(&diag, &data, &ctx);
        // task-1 is not renamed, so no depends edit needed.
        insta::assert_snapshot!(format_actions(&actions), @r#"
        [none] Renumber sequential IDs
          edit: file:///tmp/project/spec.md
            @5:10-5:16 replace `task-2`
        "#);
    }
}
