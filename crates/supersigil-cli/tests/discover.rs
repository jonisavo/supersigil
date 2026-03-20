use std::fs;

use tempfile::TempDir;

use supersigil_cli::discover_spec_files;

fn setup_fixture(dir: &TempDir) {
    let specs = dir.path().join("specs");
    fs::create_dir_all(specs.join("auth")).unwrap();
    fs::create_dir_all(specs.join("billing")).unwrap();
    fs::write(
        specs.join("auth/req.mdx"),
        "---\nsupersigil:\n  id: a\n---\n",
    )
    .unwrap();
    fs::write(
        specs.join("auth/design.mdx"),
        "---\nsupersigil:\n  id: b\n---\n",
    )
    .unwrap();
    fs::write(
        specs.join("billing/req.mdx"),
        "---\nsupersigil:\n  id: c\n---\n",
    )
    .unwrap();
    fs::write(specs.join("auth/notes.txt"), "not a spec").unwrap();
}

#[test]
fn discovers_mdx_files_matching_glob() {
    let tmp = TempDir::new().unwrap();
    setup_fixture(&tmp);

    let paths = discover_spec_files(&["specs/**/*.mdx".to_string()], tmp.path()).unwrap();

    assert_eq!(paths.len(), 3);
    assert!(
        paths
            .iter()
            .all(|p| p.extension().is_some_and(|e| e == "mdx"))
    );
}

#[test]
fn invalid_glob_returns_error() {
    let tmp = TempDir::new().unwrap();

    let err = discover_spec_files(&["[invalid".to_string()], tmp.path()).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("invalid"),
        "invalid glob should be surfaced, got: {message}",
    );
}
