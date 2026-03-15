use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;
use supersigil_core::ExamplesConfig;
use supersigil_verify::examples::executor;
use supersigil_verify::examples::types::ExampleSpec;

use crate::commands::ExamplesArgs;
use crate::error::CliError;
use crate::format::{COL_GAP, ColorConfig, OutputFormat, Token, write_cell, write_json};
use crate::loader;
use crate::scope;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ExampleEntry {
    pub doc_id: String,
    pub example_id: String,
    pub lang: String,
    pub runner: String,
    pub has_expected: bool,
    pub verifies: Vec<String>,
}

impl ExampleEntry {
    fn from_spec(spec: &ExampleSpec) -> Self {
        Self {
            doc_id: spec.doc_id.clone(),
            example_id: spec.example_id.clone(),
            lang: spec.lang.clone(),
            runner: spec.runner.clone(),
            has_expected: spec.expected.is_some(),
            verifies: spec.verifies.iter().map(ToString::to_string).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Collection
// ---------------------------------------------------------------------------

/// Collect all `Example` entries from the graph, optionally filtered by prefix.
///
/// Delegates to `executor::collect_examples` for the component walk, then
/// projects each `ExampleSpec` to the lighter display-only `ExampleEntry`.
fn collect_entries(
    graph: &supersigil_core::DocumentGraph,
    config: &ExamplesConfig,
    prefix: Option<&str>,
) -> Vec<ExampleEntry> {
    executor::collect_examples(graph, config)
        .iter()
        .filter(|spec| prefix.is_none_or(|p| spec.doc_id.starts_with(p)))
        .map(ExampleEntry::from_spec)
        .collect()
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn write_terminal_table(
    out: &mut impl Write,
    entries: &[ExampleEntry],
    color: ColorConfig,
) -> io::Result<()> {
    if entries.is_empty() {
        writeln!(out, "No examples found.")?;
        return Ok(());
    }

    // Compute column widths.
    let doc_w = entries
        .iter()
        .map(|e| e.doc_id.len())
        .max()
        .unwrap_or(0)
        .max("DOCUMENT".len());
    let ex_w = entries
        .iter()
        .map(|e| e.example_id.len())
        .max()
        .unwrap_or(0)
        .max("EXAMPLE".len());
    let lang_w = entries
        .iter()
        .map(|e| e.lang.len())
        .max()
        .unwrap_or(0)
        .max("LANG".len());
    let runner_w = entries
        .iter()
        .map(|e| e.runner.len())
        .max()
        .unwrap_or(0)
        .max("RUNNER".len());

    // Header.
    write_cell(out, color, Token::Header, "DOCUMENT", doc_w)?;
    write!(out, "{COL_GAP}")?;
    write_cell(out, color, Token::Header, "EXAMPLE", ex_w)?;
    write!(out, "{COL_GAP}")?;
    write_cell(out, color, Token::Header, "LANG", lang_w)?;
    write!(out, "{COL_GAP}")?;
    write_cell(out, color, Token::Header, "RUNNER", runner_w)?;
    write!(out, "{COL_GAP}")?;
    write_cell(out, color, Token::Header, "EXPECTED", 8)?;
    write!(out, "{COL_GAP}")?;
    writeln!(out, "{}", color.paint(Token::Header, "VERIFIES"))?;

    for entry in entries {
        let expected_str = if entry.has_expected { "yes" } else { "no" };
        let verifies_str = entry.verifies.join(", ");

        write_cell(out, color, Token::DocId, &entry.doc_id, doc_w)?;
        write!(out, "{COL_GAP}")?;
        write!(out, "{:<ex_w$}", entry.example_id)?;
        write!(out, "{COL_GAP}")?;
        write!(out, "{:<lang_w$}", entry.lang)?;
        write!(out, "{COL_GAP}")?;
        write!(out, "{:<runner_w$}", entry.runner)?;
        write!(out, "{COL_GAP}")?;
        write!(out, "{expected_str:<8}")?;
        write!(out, "{COL_GAP}")?;
        writeln!(out, "{verifies_str}")?;
    }

    writeln!(
        out,
        "\n{} examples",
        color.paint(Token::Count, &entries.len().to_string())
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the `examples` command: list executable examples in the spec.
///
/// # Errors
///
/// Returns `CliError` if the graph cannot be loaded or output fails.
pub fn run(args: &ExamplesArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);
    let cwd = std::env::current_dir().map_err(CliError::Io)?;

    let mut entries = collect_entries(&graph, &config.examples, args.prefix.as_deref());

    // Context scoping: when no prefix and --all is not set, filter by cwd.
    if args.prefix.is_none()
        && !args.all
        && let Some(scope) =
            scope::apply_context_scope(&graph, project_root, &cwd, "examples", color)
    {
        entries.retain(|e| scope.contains(&e.doc_id));
    }

    entries.sort_by(|a, b| {
        a.doc_id
            .cmp(&b.doc_id)
            .then_with(|| a.example_id.cmp(&b.example_id))
    });

    match args.format {
        OutputFormat::Json => write_json(&entries).map_err(CliError::Io)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            write_terminal_table(&mut out, &entries, color).map_err(CliError::Io)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use supersigil_core::SourcePosition;
    use supersigil_evidence::VerifiableRef;
    use supersigil_verify::examples::types::ExpectedSpec;

    fn no_color() -> ColorConfig {
        ColorConfig::no_color()
    }

    fn make_entry(
        doc_id: &str,
        example_id: &str,
        lang: &str,
        runner: &str,
        has_expected: bool,
        verifies: &[&str],
    ) -> ExampleEntry {
        ExampleEntry {
            doc_id: doc_id.to_owned(),
            example_id: example_id.to_owned(),
            lang: lang.to_owned(),
            runner: runner.to_owned(),
            has_expected,
            verifies: verifies.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn terminal_table_empty_shows_message() {
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &[], no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("No examples found"), "got: {output}");
    }

    #[test]
    fn terminal_table_shows_header_and_rows() {
        let entries = vec![make_entry(
            "req/auth",
            "auth-1",
            "sh",
            "sh",
            true,
            &["req/auth#crit-1"],
        )];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains("DOCUMENT"), "should show header: {output}");
        assert!(output.contains("EXAMPLE"), "should show header: {output}");
        assert!(output.contains("LANG"), "should show header: {output}");
        assert!(output.contains("RUNNER"), "should show header: {output}");
        assert!(output.contains("EXPECTED"), "should show header: {output}");
        assert!(output.contains("VERIFIES"), "should show header: {output}");
        assert!(output.contains("req/auth"), "should show doc_id: {output}");
        assert!(
            output.contains("auth-1"),
            "should show example_id: {output}"
        );
        assert!(output.contains("yes"), "should show has_expected: {output}");
        assert!(
            output.contains("req/auth#crit-1"),
            "should show verifies: {output}"
        );
        assert!(output.contains("1 examples"), "should show count: {output}");
    }

    #[test]
    fn terminal_table_no_expected_shows_no() {
        let entries = vec![make_entry("doc/a", "ex-1", "http", "http", false, &[])];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("no"),
            "should show 'no' for expected: {output}"
        );
    }

    #[test]
    fn terminal_table_multiple_verifies_joined() {
        let entries = vec![make_entry(
            "design/api",
            "api-test",
            "http",
            "http",
            true,
            &["design/api#crit-2", "design/api#crit-3"],
        )];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("design/api#crit-2, design/api#crit-3"),
            "should join verifies with comma: {output}"
        );
    }

    #[test]
    fn terminal_table_aligns_columns() {
        let entries = vec![
            make_entry(
                "executable-examples/req",
                "fixture-examples-json",
                "sh",
                "sh",
                true,
                &["executable-examples/req#req-5-1"],
            ),
            make_entry(
                "executable-examples/req",
                "fixture-lint",
                "sh",
                "sh",
                true,
                &["executable-examples/req#req-1-1"],
            ),
        ];
        let mut buf = Vec::new();
        write_terminal_table(&mut buf, &entries, no_color()).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        let header_example_pos = lines[0].find("EXAMPLE").unwrap();
        let row1_example_pos = lines[1].find("fixture-examples-json").unwrap();
        let row2_example_pos = lines[2].find("fixture-lint").unwrap();
        assert_eq!(
            header_example_pos, row1_example_pos,
            "EXAMPLE column misaligned:\n{output}"
        );
        assert_eq!(
            header_example_pos, row2_example_pos,
            "EXAMPLE column misaligned:\n{output}"
        );

        let header_expected_pos = lines[0].find("EXPECTED").unwrap();
        let row1_expected_pos = lines[1].find("yes").unwrap();
        let row2_expected_pos = lines[2].find("yes").unwrap();
        assert_eq!(
            header_expected_pos, row1_expected_pos,
            "EXPECTED column misaligned:\n{output}"
        );
        assert_eq!(
            header_expected_pos, row2_expected_pos,
            "EXPECTED column misaligned:\n{output}"
        );
    }

    #[test]
    fn from_spec_maps_all_fields() {
        let spec = ExampleSpec {
            doc_id: "req/auth".to_string(),
            example_id: "auth-1".to_string(),
            lang: "sh".to_string(),
            runner: "sh".to_string(),
            verifies: vec![VerifiableRef {
                doc_id: "req/auth".to_string(),
                target_id: "crit-1".to_string(),
            }],
            code: String::new(),
            expected: Some(ExpectedSpec {
                status: None,
                format: supersigil_verify::examples::types::MatchFormat::Text,
                contains: None,
                body: None,
                body_span: None,
            }),
            timeout: 30,
            env: vec![],
            setup: None,
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            source_path: PathBuf::from("specs/test.mdx"),
        };

        let entry = ExampleEntry::from_spec(&spec);
        assert_eq!(entry.doc_id, "req/auth");
        assert_eq!(entry.example_id, "auth-1");
        assert_eq!(entry.lang, "sh");
        assert_eq!(entry.runner, "sh");
        assert!(entry.has_expected);
        assert_eq!(entry.verifies, vec!["req/auth#crit-1"]);
    }

    #[test]
    fn from_spec_no_expected() {
        let spec = ExampleSpec {
            doc_id: "doc/a".to_string(),
            example_id: "ex-1".to_string(),
            lang: "http".to_string(),
            runner: "http".to_string(),
            verifies: vec![],
            code: String::new(),
            expected: None,
            timeout: 30,
            env: vec![],
            setup: None,
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            source_path: PathBuf::from("specs/test.mdx"),
        };

        let entry = ExampleEntry::from_spec(&spec);
        assert!(!entry.has_expected);
        assert!(entry.verifies.is_empty());
    }

    #[test]
    fn json_output_has_correct_fields() {
        let entries = vec![make_entry(
            "req/auth",
            "auth-1",
            "sh",
            "sh",
            true,
            &["req/auth#crit-1"],
        )];
        let json = serde_json::to_value(&entries).unwrap();
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let obj = &arr[0];
        assert_eq!(obj["doc_id"], "req/auth");
        assert_eq!(obj["example_id"], "auth-1");
        assert_eq!(obj["lang"], "sh");
        assert_eq!(obj["runner"], "sh");
        assert_eq!(obj["has_expected"], true);
        assert_eq!(obj["verifies"][0], "req/auth#crit-1");
    }

    #[test]
    fn json_output_empty_verifies() {
        let entries = vec![make_entry("doc/a", "ex-1", "sh", "sh", false, &[])];
        let json = serde_json::to_value(&entries).unwrap();
        let obj = &json.as_array().unwrap()[0];
        assert!(
            obj["verifies"].as_array().unwrap().is_empty(),
            "verifies should be empty array"
        );
    }
}
