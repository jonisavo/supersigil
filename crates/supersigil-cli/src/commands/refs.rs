use std::collections::HashSet;
use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use supersigil_core::{CRITERION, DocumentGraph, glob_prefix};

use crate::commands::RefsArgs;
use crate::error::CliError;
use crate::format::{self, ColorConfig, OutputFormat, Token, write_json};
use crate::loader;

/// Maximum body text length shown in terminal mode.
const MAX_BODY_LEN: usize = 72;

#[derive(Debug, Serialize)]
pub struct CriterionRefEntry {
    #[serde(rename = "ref")]
    pub ref_string: String,
    pub doc_id: String,
    pub criterion_id: String,
    pub body_text: Option<String>,
}

/// Filter entries by an optional document ID prefix.
fn filter_by_prefix(
    mut entries: Vec<CriterionRefEntry>,
    prefix: Option<&String>,
) -> Vec<CriterionRefEntry> {
    if let Some(prefix) = prefix {
        entries.retain(|e| e.doc_id.starts_with(prefix.as_str()));
    }
    entries
}

/// Check whether `cwd` falls within the non-wildcard prefix of `glob_str`.
///
/// The prefix is the longest directory path that contains no glob meta
/// characters (`*`, `?`, `[`). If `cwd` (relative to `project_root`)
/// starts with that prefix, the glob is considered relevant.
fn cwd_matches_glob(cwd: &Path, project_root: &Path, glob_str: &str) -> bool {
    let Ok(relative_cwd) = cwd.strip_prefix(project_root) else {
        return false;
    };

    let prefix = glob_prefix(glob_str);
    if prefix.is_empty() {
        // Glob like `**/*.rs` — the prefix is the project root itself.
        // Any cwd inside the project matches.
        return true;
    }

    let prefix_path = Path::new(&prefix);
    // cwd is inside (or equal to) the prefix directory
    relative_cwd.starts_with(prefix_path)
}

/// Determine which document IDs are relevant based on cwd and `TrackedFiles` globs.
///
/// Returns `Some(set)` when at least one document's `TrackedFiles` globs match
/// the current working directory, or `None` when no documents match (caller
/// should fall back to showing everything).
///
/// After matching `TrackedFiles`, the scope is expanded by following `<Implements>`
/// relationships from matched documents. This ensures that when a design doc
/// has `TrackedFiles` but the criteria live on its requirement doc, the criteria
/// are still included in the scoped output.
fn resolve_context_scope(
    graph: &DocumentGraph,
    project_root: &Path,
    cwd: &Path,
) -> Option<HashSet<String>> {
    let mut matched_doc_ids = HashSet::new();

    for (doc_id, globs) in graph.all_tracked_files() {
        for glob_pattern in globs {
            if cwd_matches_glob(cwd, project_root, glob_pattern) {
                matched_doc_ids.insert(doc_id.to_owned());
                break;
            }
        }
    }

    if matched_doc_ids.is_empty() {
        return None;
    }

    // Expand scope: for each matched doc, also include documents it implements.
    let expansion: Vec<String> = matched_doc_ids
        .iter()
        .flat_map(|doc_id| graph.implements_targets(doc_id))
        .map(str::to_owned)
        .collect();
    matched_doc_ids.extend(expansion);

    Some(matched_doc_ids)
}

/// Filter entries to only those whose `doc_id` is in the allowed set.
fn filter_by_scope(
    mut entries: Vec<CriterionRefEntry>,
    scope: &HashSet<String>,
) -> Vec<CriterionRefEntry> {
    entries.retain(|e| scope.contains(&e.doc_id));
    entries
}

/// Run the `refs` command: list criterion refs in the project.
///
/// # Errors
///
/// Returns `CliError` if the graph cannot be loaded or output fails.
pub fn run(args: &RefsArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);
    let cwd = std::env::current_dir().map_err(CliError::Io)?;

    let mut entries: Vec<CriterionRefEntry> = graph
        .criteria()
        .filter(|(_, _, comp)| comp.name == CRITERION)
        .map(|(doc_id, criterion_id, comp)| CriterionRefEntry {
            ref_string: format!("{doc_id}#{criterion_id}"),
            doc_id: doc_id.to_owned(),
            criterion_id: criterion_id.to_owned(),
            body_text: comp.body_text.clone(),
        })
        .collect();

    // Context scoping: when no prefix and --all is not set, filter by cwd.
    if args.prefix.is_none() && !args.all {
        match resolve_context_scope(&graph, project_root, &cwd) {
            Some(scope) => {
                let doc_ids: Vec<&str> = {
                    let mut v: Vec<&str> = scope.iter().map(String::as_str).collect();
                    v.sort_unstable();
                    v
                };
                format::hint(
                    color,
                    &format!(
                        "showing refs scoped to: {}. Use --all to show everything.",
                        doc_ids.join(", "),
                    ),
                );
                entries = filter_by_scope(entries, &scope);
            }
            None => {
                format::hint(
                    color,
                    "no TrackedFiles match the current directory; showing all refs.",
                );
            }
        }
    }

    entries = filter_by_prefix(entries, args.prefix.as_ref());
    entries.sort_by(|a, b| a.ref_string.cmp(&b.ref_string));

    match args.format {
        OutputFormat::Json => write_json(&entries)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            write_terminal_table(&mut out, &entries, color)?;
        }
    }

    Ok(())
}

/// Truncate body text for terminal display.
fn truncate_body(text: &str, max_len: usize) -> String {
    let text = text.replace('\n', " ");
    if text.len() <= max_len {
        text
    } else {
        let boundary = text.floor_char_boundary(max_len - 3);
        format!("{}...", &text[..boundary])
    }
}

const COL_GAP: &str = "  ";

fn write_terminal_table(
    out: &mut impl Write,
    entries: &[CriterionRefEntry],
    color: ColorConfig,
) -> io::Result<()> {
    if entries.is_empty() {
        writeln!(out, "No criterion refs found.")?;
        return Ok(());
    }

    // Compute column width for ref strings.
    let ref_width = entries
        .iter()
        .map(|e| e.ref_string.len())
        .max()
        .unwrap_or(0);

    for entry in entries {
        let ref_painted = color.paint(Token::DocId, &entry.ref_string);
        let ref_pad = ref_width.saturating_sub(entry.ref_string.len());
        write!(out, "{ref_painted}{:>pad$}{COL_GAP}", "", pad = ref_pad)?;
        match &entry.body_text {
            Some(text) => {
                let truncated = truncate_body(text, MAX_BODY_LEN);
                writeln!(out, "{}", color.paint(Token::Path, &truncated))?;
            }
            None => writeln!(out)?,
        }
    }

    writeln!(
        out,
        "\n{} refs",
        color.paint(Token::Count, &entries.len().to_string())
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ColorChoice;

    fn entry(
        ref_string: &str,
        doc_id: &str,
        criterion_id: &str,
        body_text: Option<&str>,
    ) -> CriterionRefEntry {
        CriterionRefEntry {
            ref_string: ref_string.to_owned(),
            doc_id: doc_id.to_owned(),
            criterion_id: criterion_id.to_owned(),
            body_text: body_text.map(str::to_owned),
        }
    }

    fn no_color() -> ColorConfig {
        ColorConfig::resolve(ColorChoice::Never)
    }

    #[test]
    fn terminal_table_shows_refs_sorted() {
        let entries = vec![
            entry(
                "auth/req/login#login-succeeds",
                "auth/req/login",
                "login-succeeds",
                Some("WHEN valid email and password are submitted"),
            ),
            entry(
                "auth/req/login#login-fails",
                "auth/req/login",
                "login-fails",
                Some("WHEN invalid credentials are provided"),
            ),
        ];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("auth/req/login#login-succeeds"),
            "should contain ref: {output}"
        );
        assert!(
            output.contains("auth/req/login#login-fails"),
            "should contain ref: {output}"
        );
        assert!(
            output.contains("WHEN valid email"),
            "should contain body text: {output}"
        );
        assert!(output.contains("2 refs"), "should show count: {output}");
    }

    #[test]
    fn terminal_table_aligns_columns() {
        let entries = vec![
            entry("short/doc#a", "short/doc", "a", Some("body a")),
            entry(
                "much/longer/doc#criterion-b",
                "much/longer/doc",
                "criterion-b",
                Some("body b"),
            ),
        ];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        // Both body texts should start at the same column.
        let pos_a = lines[0].find("body a").unwrap();
        let pos_b = lines[1].find("body b").unwrap();
        assert_eq!(
            pos_a, pos_b,
            "body text columns should be aligned:\n{output}"
        );
    }

    #[test]
    fn terminal_table_truncates_long_body() {
        let long_body = "A".repeat(100);
        let entries = vec![entry("doc#a", "doc", "a", Some(&long_body))];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("..."),
            "should truncate with ellipsis: {output}"
        );
        // The full 100-char body should NOT appear.
        assert!(
            !output.contains(&long_body),
            "should not contain full body: {output}"
        );
    }

    #[test]
    fn terminal_table_handles_no_body_text() {
        let entries = vec![entry("doc#a", "doc", "a", None)];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("doc#a"), "should contain ref: {output}");
        assert!(output.contains("1 refs"), "should show count: {output}");
    }

    #[test]
    fn terminal_table_empty_shows_message() {
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &[], no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("No criterion refs found"), "got: {output}");
    }

    #[test]
    fn json_output_has_correct_fields() {
        let entries = vec![entry(
            "auth/req/login#login-succeeds",
            "auth/req/login",
            "login-succeeds",
            Some("WHEN valid email and password are submitted"),
        )];
        let json = serde_json::to_value(&entries).unwrap();
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let obj = &arr[0];
        assert_eq!(obj["ref"], "auth/req/login#login-succeeds");
        assert_eq!(obj["doc_id"], "auth/req/login");
        assert_eq!(obj["criterion_id"], "login-succeeds");
        assert_eq!(
            obj["body_text"],
            "WHEN valid email and password are submitted"
        );
    }

    #[test]
    fn json_output_null_body_text() {
        let entries = vec![entry("doc#a", "doc", "a", None)];
        let json = serde_json::to_value(&entries).unwrap();
        let obj = &json.as_array().unwrap()[0];
        assert!(obj["body_text"].is_null(), "body_text should be null");
    }

    #[test]
    fn json_output_empty_array() {
        let entries: Vec<CriterionRefEntry> = vec![];
        let json = serde_json::to_value(&entries).unwrap();
        let arr = json.as_array().unwrap();
        assert!(arr.is_empty());
    }

    #[test]
    fn truncate_body_short_text_unchanged() {
        assert_eq!(truncate_body("hello", 72), "hello");
    }

    #[test]
    fn truncate_body_exact_length_unchanged() {
        let text = "A".repeat(72);
        assert_eq!(truncate_body(&text, 72), text);
    }

    #[test]
    fn truncate_body_long_text_truncated() {
        let text = "A".repeat(100);
        let result = truncate_body(&text, 72);
        assert_eq!(result.len(), 72);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_body_multibyte_utf8_does_not_panic() {
        // 'é' is 2 bytes; ensure we don't slice in the middle of it.
        let text = "é".repeat(50); // 100 bytes, 50 chars
        let result = truncate_body(&text, 10);
        assert!(result.ends_with("..."));
        // Must be valid UTF-8 (would panic on String construction otherwise).
        assert!(result.len() <= 10 + 3); // boundary may be shorter + "..."
    }

    #[test]
    fn truncate_body_replaces_newlines() {
        let text = "line one\nline two";
        let result = truncate_body(text, 72);
        assert_eq!(result, "line one line two");
    }

    #[test]
    fn filter_entries_by_prefix() {
        let entries = vec![
            entry(
                "auth/req/login#login-succeeds",
                "auth/req/login",
                "login-succeeds",
                Some("login works"),
            ),
            entry(
                "auth/req/login#login-fails",
                "auth/req/login",
                "login-fails",
                Some("login fails"),
            ),
            entry(
                "billing/req/invoice#inv-created",
                "billing/req/invoice",
                "inv-created",
                Some("invoice created"),
            ),
        ];
        let filtered = filter_by_prefix(entries, Some(&"auth/".to_string()));
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|e| e.doc_id.starts_with("auth/")));
    }

    #[test]
    fn filter_entries_no_prefix_returns_all() {
        let entries = vec![
            entry("auth/req#a", "auth/req", "a", Some("a")),
            entry("billing/req#b", "billing/req", "b", Some("b")),
        ];
        let filtered = filter_by_prefix(entries, None);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn filter_entries_prefix_no_match_returns_empty() {
        let entries = vec![
            entry("auth/req#a", "auth/req", "a", Some("a")),
            entry("billing/req#b", "billing/req", "b", Some("b")),
        ];
        let filtered = filter_by_prefix(entries, Some(&"zzz/".to_string()));
        assert!(filtered.is_empty());
    }

    // -----------------------------------------------------------------------
    // glob_prefix tests (reuses supersigil_core::glob_prefix)
    // -----------------------------------------------------------------------

    #[test]
    fn glob_prefix_extracts_directory_prefix() {
        // Note: glob_prefix includes trailing `/`
        assert_eq!(glob_prefix("src/auth/**/*.rs"), "src/auth/");
    }

    #[test]
    fn glob_prefix_wildcard_at_root() {
        assert_eq!(glob_prefix("**/*.rs"), "");
    }

    #[test]
    fn glob_prefix_no_wildcards_takes_parent() {
        assert_eq!(glob_prefix("src/lib.rs"), "src/");
    }

    #[test]
    fn glob_prefix_nested_path_with_glob() {
        assert_eq!(glob_prefix("crates/core/src/**/*.rs"), "crates/core/src/");
    }

    // -----------------------------------------------------------------------
    // cwd_matches_glob tests
    // -----------------------------------------------------------------------

    #[test]
    fn cwd_matches_glob_cwd_equals_stem() {
        let root = Path::new("/project");
        let cwd = Path::new("/project/src/auth");
        assert!(cwd_matches_glob(cwd, root, "src/auth/**/*.rs"));
    }

    #[test]
    fn cwd_matches_glob_cwd_inside_stem() {
        let root = Path::new("/project");
        let cwd = Path::new("/project/src/auth/handlers");
        assert!(cwd_matches_glob(cwd, root, "src/auth/**/*.rs"));
    }

    #[test]
    fn cwd_matches_glob_cwd_above_stem_does_not_match() {
        let root = Path::new("/project");
        let cwd = Path::new("/project/src");
        assert!(!cwd_matches_glob(cwd, root, "src/auth/**/*.rs"));
    }

    #[test]
    fn cwd_matches_glob_cwd_at_project_root_does_not_match() {
        let root = Path::new("/project");
        let cwd = Path::new("/project");
        assert!(!cwd_matches_glob(cwd, root, "src/auth/**/*.rs"));
    }

    #[test]
    fn cwd_matches_glob_wildcard_at_root_matches_any_cwd() {
        let root = Path::new("/project");
        // Glob `**/*.rs` has empty stem → any cwd inside project matches.
        assert!(cwd_matches_glob(Path::new("/project"), root, "**/*.rs"));
        assert!(cwd_matches_glob(Path::new("/project/src"), root, "**/*.rs"));
        assert!(cwd_matches_glob(
            Path::new("/project/src/auth"),
            root,
            "**/*.rs"
        ));
    }

    #[test]
    fn cwd_matches_glob_cwd_outside_project_does_not_match() {
        let root = Path::new("/project");
        let cwd = Path::new("/other/place");
        assert!(!cwd_matches_glob(cwd, root, "src/auth/**/*.rs"));
    }

    #[test]
    fn cwd_matches_glob_sibling_directory_does_not_match() {
        let root = Path::new("/project");
        let cwd = Path::new("/project/src/billing");
        assert!(!cwd_matches_glob(cwd, root, "src/auth/**/*.rs"));
    }

    // -----------------------------------------------------------------------
    // resolve_context_scope tests
    // -----------------------------------------------------------------------

    use supersigil_verify::test_helpers::{
        build_test_graph, make_acceptance_criteria, make_criterion, make_doc, make_implements,
        make_tracked_files,
    };

    #[test]
    fn resolve_scope_cwd_inside_tracked_area() {
        let docs = vec![
            make_doc(
                "design/auth",
                vec![
                    make_tracked_files("src/auth/**/*.rs", 5),
                    make_criterion("auth-1", 10),
                ],
            ),
            make_doc(
                "design/billing",
                vec![
                    make_tracked_files("src/billing/**/*.rs", 5),
                    make_criterion("bill-1", 10),
                ],
            ),
        ];
        let graph = build_test_graph(docs);

        let root = Path::new("/project");
        let cwd = Path::new("/project/src/auth");
        let scope = resolve_context_scope(&graph, root, cwd);

        assert!(scope.is_some(), "should match design/auth");
        let scope = scope.unwrap();
        assert!(scope.contains("design/auth"), "scope: {scope:?}");
        assert!(!scope.contains("design/billing"), "scope: {scope:?}");
    }

    #[test]
    fn resolve_scope_cwd_outside_all_tracked_areas_returns_none() {
        let docs = vec![make_doc(
            "design/auth",
            vec![
                make_tracked_files("src/auth/**/*.rs", 5),
                make_criterion("auth-1", 10),
            ],
        )];
        let graph = build_test_graph(docs);

        let root = Path::new("/project");
        let cwd = Path::new("/project/tests");
        let scope = resolve_context_scope(&graph, root, cwd);

        assert!(scope.is_none(), "no documents should match");
    }

    #[test]
    fn resolve_scope_empty_tracked_files_returns_none() {
        // Document with no TrackedFiles component at all.
        let docs = vec![make_doc("design/auth", vec![make_criterion("auth-1", 10)])];
        let graph = build_test_graph(docs);

        let root = Path::new("/project");
        let cwd = Path::new("/project/src/auth");
        let scope = resolve_context_scope(&graph, root, cwd);

        assert!(scope.is_none(), "no tracked files means no match");
    }

    #[test]
    fn resolve_scope_multiple_docs_can_match() {
        let docs = vec![
            make_doc(
                "design/auth",
                vec![
                    make_tracked_files("src/shared/**/*.rs", 5),
                    make_criterion("auth-1", 10),
                ],
            ),
            make_doc(
                "design/billing",
                vec![
                    make_tracked_files("src/shared/**/*.rs", 5),
                    make_criterion("bill-1", 10),
                ],
            ),
        ];
        let graph = build_test_graph(docs);

        let root = Path::new("/project");
        let cwd = Path::new("/project/src/shared");
        let scope = resolve_context_scope(&graph, root, cwd);

        assert!(scope.is_some());
        let scope = scope.unwrap();
        assert_eq!(scope.len(), 2, "both docs share the same tracked area");
    }

    #[test]
    fn resolve_scope_follows_implements_to_include_req_docs() {
        // design/auth has TrackedFiles and Implements refs="req/auth".
        // req/auth has criteria but no TrackedFiles.
        // When cwd matches design/auth's tracked area, scope should include
        // both design/auth AND req/auth.
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("crit-1", 10)],
                    9,
                )],
            ),
            make_doc(
                "design/auth",
                vec![
                    make_implements("req/auth", 1),
                    make_tracked_files("src/auth/**/*.rs", 5),
                ],
            ),
        ];
        let graph = build_test_graph(docs);

        let root = Path::new("/project");
        let cwd = Path::new("/project/src/auth");
        let scope = resolve_context_scope(&graph, root, cwd);

        assert!(scope.is_some(), "should match design/auth's TrackedFiles");
        let scope = scope.unwrap();
        assert!(
            scope.contains("design/auth"),
            "should include design/auth: {scope:?}"
        );
        assert!(
            scope.contains("req/auth"),
            "should include req/auth via Implements: {scope:?}"
        );
    }

    #[test]
    fn resolve_scope_implements_expansion_does_not_include_unrelated_docs() {
        let docs = vec![
            make_doc(
                "req/auth",
                vec![make_acceptance_criteria(
                    vec![make_criterion("crit-1", 10)],
                    9,
                )],
            ),
            make_doc(
                "req/billing",
                vec![make_acceptance_criteria(
                    vec![make_criterion("bill-1", 10)],
                    9,
                )],
            ),
            make_doc(
                "design/auth",
                vec![
                    make_implements("req/auth", 1),
                    make_tracked_files("src/auth/**/*.rs", 5),
                ],
            ),
        ];
        let graph = build_test_graph(docs);

        let root = Path::new("/project");
        let cwd = Path::new("/project/src/auth");
        let scope = resolve_context_scope(&graph, root, cwd).unwrap();

        assert!(scope.contains("req/auth"), "scope: {scope:?}");
        assert!(
            !scope.contains("req/billing"),
            "should NOT include unrelated req doc: {scope:?}"
        );
    }

    // -----------------------------------------------------------------------
    // filter_by_scope tests
    // -----------------------------------------------------------------------

    #[test]
    fn filter_by_scope_keeps_matching_doc_ids() {
        let entries = vec![
            entry("auth/req#a", "auth/req", "a", Some("a")),
            entry("billing/req#b", "billing/req", "b", Some("b")),
        ];
        let scope: HashSet<String> = ["auth/req".to_owned()].into_iter().collect();
        let filtered = filter_by_scope(entries, &scope);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].doc_id, "auth/req");
    }

    #[test]
    fn filter_by_scope_empty_scope_returns_empty() {
        let entries = vec![entry("auth/req#a", "auth/req", "a", Some("a"))];
        let scope: HashSet<String> = HashSet::new();
        let filtered = filter_by_scope(entries, &scope);
        assert!(filtered.is_empty());
    }
}
