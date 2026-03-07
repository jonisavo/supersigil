use std::io::{self, Write};
use std::path::Path;

use serde::Serialize;

use crate::commands::LsArgs;
use crate::error::CliError;
use crate::format::{ColorConfig, OutputFormat, Token, status_token, write_json};
use crate::loader;

#[derive(Serialize)]
struct DocEntry {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    doc_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    path: String,
}

/// Run the `ls` command: list documents with optional filters.
///
/// # Errors
///
/// Returns `CliError` if the graph cannot be loaded or output fails.
pub fn run(args: &LsArgs, config_path: &Path, color: ColorConfig) -> Result<(), CliError> {
    let (_config, graph) = loader::load_graph(config_path)?;
    let project_root = loader::project_root(config_path);

    let mut entries: Vec<DocEntry> = graph
        .documents()
        .filter(|(_, doc)| {
            if let Some(ref t) = args.doc_type
                && doc.frontmatter.doc_type.as_deref() != Some(t.as_str())
            {
                return false;
            }
            if let Some(ref s) = args.status
                && doc.frontmatter.status.as_deref() != Some(s.as_str())
            {
                return false;
            }
            if let Some(ref p) = args.project
                && graph.doc_project(&doc.frontmatter.id) != Some(p.as_str())
            {
                return false;
            }
            true
        })
        .map(|(_, doc)| DocEntry {
            id: doc.frontmatter.id.clone(),
            doc_type: doc.frontmatter.doc_type.clone(),
            status: doc.frontmatter.status.clone(),
            path: doc.path.display().to_string(),
        })
        .collect();

    // Sort for stable output
    entries.sort_by(|a, b| a.id.cmp(&b.id));

    match args.format {
        OutputFormat::Json => write_json(&entries)?,
        OutputFormat::Terminal => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            write_table(&mut out, &entries, project_root, color)?;
        }
    }

    Ok(())
}

const HEADERS: [&str; 4] = ["ID", "Type", "Status", "Path"];
const COL_GAP: &str = "  ";

/// Write a colored, padded cell. Pads text to `width` first, then paints.
fn write_cell(
    out: &mut impl Write,
    color: ColorConfig,
    token: Token,
    text: &str,
    width: usize,
) -> io::Result<()> {
    // Paint the text, then pad with trailing spaces (outside the style).
    write!(out, "{}", color.paint(token, text))?;
    let pad = width.saturating_sub(text.len());
    for _ in 0..pad {
        write!(out, " ")?;
    }
    Ok(())
}

fn write_table(
    out: &mut impl Write,
    entries: &[DocEntry],
    project_root: &Path,
    color: ColorConfig,
) -> io::Result<()> {
    if entries.is_empty() {
        writeln!(out, "No documents found.")?;
        return Ok(());
    }

    // Compute relative paths for display
    let rel_paths: Vec<String> = entries
        .iter()
        .map(|e| {
            Path::new(&e.path)
                .strip_prefix(project_root)
                .map_or_else(|_| e.path.clone(), |p| p.display().to_string())
        })
        .collect();

    // Column widths (padded columns; last column is unpadded)
    let w = [
        entries
            .iter()
            .map(|e| e.id.len())
            .max()
            .unwrap_or(0)
            .max(HEADERS[0].len()),
        entries
            .iter()
            .map(|e| e.doc_type.as_deref().unwrap_or("-").len())
            .max()
            .unwrap_or(0)
            .max(HEADERS[1].len()),
        entries
            .iter()
            .map(|e| e.status.as_deref().unwrap_or("-").len())
            .max()
            .unwrap_or(0)
            .max(HEADERS[2].len()),
        rel_paths
            .iter()
            .map(String::len)
            .max()
            .unwrap_or(0)
            .max(HEADERS[3].len()),
    ];

    // Header
    write!(
        out,
        "{}",
        color.paint(Token::Header, &format!("{:<w0$}", HEADERS[0], w0 = w[0]))
    )?;
    write!(out, "{COL_GAP}")?;
    write!(
        out,
        "{}",
        color.paint(Token::Header, &format!("{:<w1$}", HEADERS[1], w1 = w[1]))
    )?;
    write!(out, "{COL_GAP}")?;
    write!(
        out,
        "{}",
        color.paint(Token::Header, &format!("{:<w2$}", HEADERS[2], w2 = w[2]))
    )?;
    write!(out, "{COL_GAP}")?;
    writeln!(out, "{}", color.paint(Token::Header, HEADERS[3]))?;

    // Separator
    let sep = if color.use_unicode() { "─" } else { "-" };
    writeln!(
        out,
        "{}{COL_GAP}{}{COL_GAP}{}{COL_GAP}{}",
        sep.repeat(w[0]),
        sep.repeat(w[1]),
        sep.repeat(w[2]),
        sep.repeat(w[3]),
    )?;

    // Rows
    for (entry, rel_path) in entries.iter().zip(&rel_paths) {
        let doc_type = entry.doc_type.as_deref().unwrap_or("-");
        let status = entry.status.as_deref().unwrap_or("-");
        write_cell(out, color, Token::DocId, &entry.id, w[0])?;
        write!(out, "{COL_GAP}")?;
        write_cell(out, color, Token::DocType, doc_type, w[1])?;
        write!(out, "{COL_GAP}")?;
        write_cell(out, color, status_token(status), status, w[2])?;
        write!(out, "{COL_GAP}")?;
        writeln!(out, "{}", color.paint(Token::Path, rel_path))?;
    }

    // Count
    writeln!(
        out,
        "\n{} documents",
        color.paint(Token::Count, &entries.len().to_string())
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::ColorChoice;

    fn entry(id: &str, doc_type: &str, status: &str, path: &str) -> DocEntry {
        DocEntry {
            id: id.to_owned(),
            doc_type: Some(doc_type.to_owned()),
            status: Some(status.to_owned()),
            path: path.to_owned(),
        }
    }

    #[test]
    fn table_has_aligned_columns() {
        let entries = vec![
            entry(
                "auth/req",
                "requirement",
                "draft",
                "/proj/specs/auth/auth.req.mdx",
            ),
            entry(
                "auth/tasks",
                "tasks",
                "approved",
                "/proj/specs/auth/auth.tasks.mdx",
            ),
        ];
        let color = ColorConfig::resolve(ColorChoice::Never);
        let mut buf = Vec::new();
        write_table(&mut buf, &entries, Path::new("/proj"), color).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();

        // Header, separator, 2 data rows, blank, count = 6 lines
        assert_eq!(lines.len(), 6, "got:\n{output}");
        assert!(lines[0].starts_with("ID"), "got:\n{output}");
        assert!(lines[1].contains("---"), "ASCII separator, got:\n{output}");
        // Check column alignment: "Status" header aligns with status values
        let header_status_pos = lines[0].find("Status").unwrap();
        let row1_status_pos = lines[2].find("draft").unwrap();
        let row2_status_pos = lines[3].find("approved").unwrap();
        assert_eq!(
            header_status_pos, row1_status_pos,
            "Status column misaligned:\n{output}"
        );
        assert_eq!(
            header_status_pos, row2_status_pos,
            "Status column misaligned:\n{output}"
        );
    }

    #[test]
    fn table_uses_unicode_separator() {
        let entries = vec![entry("a/b", "design", "draft", "/p/specs/a.mdx")];
        let color = ColorConfig::resolve(ColorChoice::Always);
        let mut buf = Vec::new();
        write_table(&mut buf, &entries, Path::new("/p"), color).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains('─'),
            "expected Unicode separator, got:\n{output}"
        );
    }

    #[test]
    fn table_shows_relative_paths() {
        let entries = vec![entry(
            "cli/req",
            "requirement",
            "draft",
            "/home/user/proj/specs/cli/cli.req.mdx",
        )];
        let color = ColorConfig::resolve(ColorChoice::Never);
        let mut buf = Vec::new();
        write_table(&mut buf, &entries, Path::new("/home/user/proj"), color).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("specs/cli/cli.req.mdx"),
            "expected relative path, got:\n{output}"
        );
        assert!(
            !output.contains("/home/user/proj"),
            "should not contain absolute prefix, got:\n{output}"
        );
    }

    #[test]
    fn table_empty_shows_message() {
        let color = ColorConfig::resolve(ColorChoice::Never);
        let mut buf = Vec::new();
        write_table(&mut buf, &[], Path::new("/p"), color).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("No documents found"), "got:\n{output}");
    }

    #[test]
    fn table_shows_count() {
        let entries = vec![
            entry("a/req", "requirement", "draft", "/p/a.mdx"),
            entry("b/req", "requirement", "draft", "/p/b.mdx"),
        ];
        let color = ColorConfig::resolve(ColorChoice::Never);
        let mut buf = Vec::new();
        write_table(&mut buf, &entries, Path::new("/p"), color).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("2 documents"), "got:\n{output}");
    }
}
