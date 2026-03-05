mod generators;

use generators::arb_feature_name;
use proptest::prelude::*;
use std::path::PathBuf;
use supersigil_import::write::write_files;
use supersigil_import::{ImportError, PlannedDocument};

/// Generate a small list of `PlannedDocument`s with realistic output paths.
fn arb_planned_documents(base_dir: PathBuf) -> impl Strategy<Value = Vec<PlannedDocument>> {
    arb_feature_name().prop_flat_map(move |feature| {
        let dir = base_dir.clone();
        prop::collection::vec(
            (
                prop::sample::select(vec!["req.mdx", "design.mdx", "tasks.mdx"]),
                "[a-z ]{5,40}",
            ),
            1..=3,
        )
        .prop_map(move |entries| {
            let mut docs = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for (filename, body) in entries {
                // Deduplicate filenames within a single test case
                if !seen.insert(filename.to_string()) {
                    continue;
                }
                docs.push(PlannedDocument {
                    output_path: dir.join(&feature).join(filename),
                    document_id: format!("test/{feature}"),
                    content: format!("---\ntitle: test\n---\n{body}\n"),
                });
            }
            docs
        })
    })
}

// Feature: kiro-import, Property 20: File writing with force semantics
//
// When `force` is false and target file exists, `write_files` returns
// `FileExists` error. When `force` is true, existing files are overwritten.
// Missing output directories are created.
//
// Validates: Requirements 15.1, 15.3, 17.1
proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    #[test]
    fn prop_20_write_creates_missing_directories(
        docs in arb_planned_documents(PathBuf::from("PLACEHOLDER")),
    ) {
        let tmp = tempfile::tempdir().unwrap();
        // Rebase output paths under the temp dir
        let docs: Vec<PlannedDocument> = docs
            .into_iter()
            .map(|mut d| {
                // Strip the PLACEHOLDER prefix and rebase under tmp
                let rel = d.output_path.strip_prefix("PLACEHOLDER").unwrap().to_path_buf();
                d.output_path = tmp.path().join("out").join(rel);
                d
            })
            .collect();

        // The output directory doesn't exist yet
        let out_dir = tmp.path().join("out");
        prop_assert!(!out_dir.exists());

        let result = write_files(&docs, false);
        prop_assert!(result.is_ok(), "write_files failed: {:?}", result.err());

        // All files should now exist with correct content
        let written = result.unwrap();
        prop_assert_eq!(written.len(), docs.len());
        for doc in &docs {
            prop_assert!(doc.output_path.exists(), "File not created: {:?}", doc.output_path);
            let on_disk = std::fs::read_to_string(&doc.output_path).unwrap();
            prop_assert_eq!(&on_disk, &doc.content);
        }
    }

    #[test]
    fn prop_20_write_returns_file_exists_without_force(
        docs in arb_planned_documents(PathBuf::from("PLACEHOLDER")),
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let docs: Vec<PlannedDocument> = docs
            .into_iter()
            .map(|mut d| {
                let rel = d.output_path.strip_prefix("PLACEHOLDER").unwrap().to_path_buf();
                d.output_path = tmp.path().join("out").join(rel);
                d
            })
            .collect();

        // Pre-create the first file so it conflicts
        if let Some(first) = docs.first() {
            std::fs::create_dir_all(first.output_path.parent().unwrap()).unwrap();
            std::fs::write(&first.output_path, "existing content").unwrap();
        }

        let result = write_files(&docs, false);
        // Should fail with FileExists
        prop_assert!(result.is_err(), "Expected FileExists error but got Ok");
        let err = result.unwrap_err();
        match &err {
            ImportError::FileExists { path } => {
                prop_assert_eq!(path, &docs[0].output_path);
            }
            other => prop_assert!(false, "Expected FileExists, got: {other}"),
        }
    }

    #[test]
    fn prop_20_write_overwrites_with_force(
        docs in arb_planned_documents(PathBuf::from("PLACEHOLDER")),
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let docs: Vec<PlannedDocument> = docs
            .into_iter()
            .map(|mut d| {
                let rel = d.output_path.strip_prefix("PLACEHOLDER").unwrap().to_path_buf();
                d.output_path = tmp.path().join("out").join(rel);
                d
            })
            .collect();

        // Pre-create all files with stale content
        for doc in &docs {
            std::fs::create_dir_all(doc.output_path.parent().unwrap()).unwrap();
            std::fs::write(&doc.output_path, "stale content").unwrap();
        }

        let result = write_files(&docs, true);
        prop_assert!(result.is_ok(), "write_files with force failed: {:?}", result.err());

        // All files should have the new content
        for doc in &docs {
            let on_disk = std::fs::read_to_string(&doc.output_path).unwrap();
            prop_assert_eq!(
                &on_disk, &doc.content,
                "File {:?} was not overwritten", doc.output_path
            );
        }

        // OutputFile entries should have correct paths and document_ids
        let written = result.unwrap();
        prop_assert_eq!(written.len(), docs.len());
        for (out, doc) in written.iter().zip(docs.iter()) {
            prop_assert_eq!(&out.path, &doc.output_path);
            prop_assert_eq!(&out.document_id, &doc.document_id);
        }
    }
}
