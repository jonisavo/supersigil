#![allow(
    dead_code,
    reason = "shared test helpers — not every test file uses every function"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub fn setup_project(dir: &Path) {
    fs::write(dir.join("supersigil.toml"), "paths = [\"specs/**/*.md\"]\n").unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
}

/// Set up a project with the Rust ecosystem plugin enabled.
pub fn setup_project_with_rust_plugin(dir: &Path) {
    fs::write(
        dir.join("supersigil.toml"),
        "paths = [\"specs/**/*.md\"]\n\n[ecosystem]\nplugins = [\"rust\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
}

/// Set up a project with the Rust ecosystem plugin and explicit test globs.
pub fn setup_project_with_rust_plugin_and_tests(dir: &Path, tests_glob: &str, extra_config: &str) {
    let extra_config = if extra_config.is_empty() {
        String::new()
    } else {
        format!("{extra_config}\n")
    };

    fs::write(
        dir.join("supersigil.toml"),
        format!(
            "paths = [\"specs/**/*.md\"]\ntests = [\"{tests_glob}\"]\n\n[ecosystem]\nplugins = [\"rust\"]\n{extra_config}"
        ),
    )
    .unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
}

pub fn write_spec(dir: &Path, name: &str, id: &str, doc_type: &str, status: &str) {
    write_spec_doc(
        dir,
        &format!("specs/{name}.md"),
        id,
        Some(doc_type),
        Some(status),
        "",
    );
}

pub fn write_spec_doc(
    dir: &Path,
    relative_path: &str,
    id: &str,
    doc_type: Option<&str>,
    status: Option<&str>,
    body: &str,
) {
    let path = dir.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    let mut content = format!("---\nsupersigil:\n  id: {id}\n");
    if let Some(doc_type) = doc_type {
        writeln!(content, "  type: {doc_type}").unwrap();
    }
    if let Some(status) = status {
        writeln!(content, "  status: {status}").unwrap();
    }
    content.push_str("---\n");
    if !body.is_empty() {
        content.push('\n');
        let wrapped = wrap_xml_body(body);
        content.push_str(&wrapped);
        if !wrapped.ends_with('\n') {
            content.push('\n');
        }
    }

    fs::write(path, content).unwrap();
}

/// Set up a project with the JS ecosystem plugin enabled.
pub fn setup_project_with_js_plugin(dir: &Path) {
    fs::write(
        dir.join("supersigil.toml"),
        "paths = [\"specs/**/*.md\"]\n\n[ecosystem]\nplugins = [\"js\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
}

/// Wrap XML component content in `supersigil-xml` fenced code blocks.
///
/// Detects XML tags (`<PascalCase`) in the body and wraps contiguous XML
/// sections in triple-backtick `supersigil-xml` fences. Markdown headings
/// and other non-XML content pass through unchanged.
///
/// If the body already contains `supersigil-xml` fences or embedded Markdown
/// code fences (e.g. `supersigil-ref`), it is returned as-is (the caller has
/// already formatted it in the new document format).
fn wrap_xml_body(body: &str) -> String {
    // Already in the new format — pass through.
    if body.contains("```supersigil-xml") || body.contains("supersigil-ref=") {
        return body.to_owned();
    }

    // If the body contains no XML-like tags, return as-is.
    if !body.contains('<') {
        return body.to_owned();
    }

    let mut result = String::new();
    let mut xml_buf = String::new();
    let mut in_xml = false;

    for line in body.lines() {
        let trimmed = line.trim();
        let is_xml_line = trimmed.starts_with('<')
            || (in_xml && !trimmed.is_empty() && !trimmed.starts_with('#'));

        if is_xml_line {
            if !in_xml {
                // Flush any preceding non-XML content.
                in_xml = true;
            }
            xml_buf.push_str(line);
            xml_buf.push('\n');
        } else {
            if in_xml {
                // Close the XML fence.
                result.push_str("```supersigil-xml\n");
                result.push_str(&xml_buf);
                result.push_str("```\n");
                xml_buf.clear();
                in_xml = false;
            }
            result.push_str(line);
            result.push('\n');
        }
    }

    if in_xml {
        result.push_str("```supersigil-xml\n");
        result.push_str(&xml_buf);
        result.push_str("```\n");
    }

    result
}
