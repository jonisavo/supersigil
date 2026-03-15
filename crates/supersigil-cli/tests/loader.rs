mod common;

use std::fs;

use supersigil_rust::verifies;
use tempfile::TempDir;

fn write_invalid_spec(dir: &std::path::Path, subpath: &str) {
    let full = dir.join(subpath);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&full, "---\nsupersigil:\n  id: broken\n").unwrap();
}

#[verifies("cli-runtime/req#req-2-3")]
#[test]
fn parse_all_returns_documents_and_errors() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");
    write_invalid_spec(tmp.path(), "specs/b.mdx");

    let config_path = tmp.path().join("supersigil.toml");
    let result = supersigil_cli::parse_all(&config_path);

    let (_, docs, errors) = result.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].frontmatter.id, "doc/a");
    assert!(!errors.is_empty());
}

#[verifies("cli-runtime/req#req-2-4")]
#[test]
fn load_graph_succeeds_with_valid_specs() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    common::write_spec(tmp.path(), "a", "doc/a", "requirements", "draft");
    common::write_spec(tmp.path(), "b", "doc/b", "requirements", "draft");

    let config_path = tmp.path().join("supersigil.toml");
    let (_config, graph) = supersigil_cli::load_graph(&config_path).unwrap();

    assert!(graph.document("doc/a").is_some());
    assert!(graph.document("doc/b").is_some());
}

#[verifies("cli-runtime/req#req-2-4")]
#[test]
fn load_graph_fails_on_parse_errors() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    write_invalid_spec(tmp.path(), "specs/bad.mdx");

    let config_path = tmp.path().join("supersigil.toml");
    let result = supersigil_cli::load_graph(&config_path);

    result.unwrap_err();
}

#[verifies("cli-runtime/req#req-1-3")]
#[test]
fn find_config_searches_upward() {
    let tmp = TempDir::new().unwrap();
    common::setup_project(tmp.path());
    let subdir = tmp.path().join("specs/nested");
    fs::create_dir_all(&subdir).unwrap();

    let found = supersigil_cli::find_config(&subdir).unwrap();
    assert_eq!(found, tmp.path().join("supersigil.toml"));
}

#[test]
fn find_config_returns_error_when_missing() {
    let tmp = TempDir::new().unwrap();
    let result = supersigil_cli::find_config(tmp.path());
    result.unwrap_err();
}

#[cfg(unix)]
#[test]
fn find_config_propagates_metadata_errors() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    symlink("supersigil.toml", tmp.path().join("supersigil.toml")).unwrap();

    let err = supersigil_cli::find_config(tmp.path()).unwrap_err();
    assert!(matches!(err, supersigil_cli::error::CliError::Io(_)));
}
