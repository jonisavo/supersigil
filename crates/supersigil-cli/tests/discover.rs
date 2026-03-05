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
fn no_matches_returns_empty_vec() {
    let tmp = TempDir::new().unwrap();
    let paths = discover_spec_files(&["specs/**/*.mdx".to_string()], tmp.path()).unwrap();

    assert!(paths.is_empty());
}

#[test]
fn multiple_globs_combined() {
    let tmp = TempDir::new().unwrap();
    setup_fixture(&tmp);
    fs::create_dir_all(tmp.path().join("extra")).unwrap();
    fs::write(
        tmp.path().join("extra/doc.mdx"),
        "---\nsupersigil:\n  id: d\n---\n",
    )
    .unwrap();

    let paths = discover_spec_files(
        &["specs/**/*.mdx".to_string(), "extra/**/*.mdx".to_string()],
        tmp.path(),
    )
    .unwrap();

    assert_eq!(paths.len(), 4);
}

#[test]
fn overlapping_globs_are_deduplicated() {
    let tmp = TempDir::new().unwrap();
    setup_fixture(&tmp);

    let paths = discover_spec_files(
        &["specs/**/*.mdx".to_string(), "specs/auth/*.mdx".to_string()],
        tmp.path(),
    )
    .unwrap();

    assert_eq!(paths.len(), 3);
    assert!(paths.windows(2).all(|pair| pair[0] <= pair[1]));
}
