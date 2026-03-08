#![allow(
    dead_code,
    reason = "shared test helpers — not every test file uses every function"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub fn setup_project(dir: &Path) {
    fs::write(
        dir.join("supersigil.toml"),
        "paths = [\"specs/**/*.mdx\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
}

/// Set up a project with the Rust ecosystem plugin enabled.
pub fn setup_project_with_rust_plugin(dir: &Path) {
    fs::write(
        dir.join("supersigil.toml"),
        "paths = [\"specs/**/*.mdx\"]\n\n[ecosystem]\nplugins = [\"rust\"]\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("specs")).unwrap();
}

pub fn write_spec(dir: &Path, name: &str, id: &str, doc_type: &str, status: &str) {
    write_mdx(
        dir,
        &format!("specs/{name}.mdx"),
        id,
        Some(doc_type),
        Some(status),
        "",
    );
}

pub fn write_mdx(
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
        content.push_str(body);
        if !body.ends_with('\n') {
            content.push('\n');
        }
    }

    fs::write(path, content).unwrap();
}
