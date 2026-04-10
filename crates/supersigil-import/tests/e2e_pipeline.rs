//! End-to-end pipeline tests for Kiro spec import.

mod common;

use common::{config_for, workspace_root};
use supersigil_core::{ComponentDefs, Config, ParseResult, build_graph};
use supersigil_import::{ImportPlan, plan_kiro_import};
use supersigil_parser::parse_file;
use tempfile::TempDir;

fn plan_real_specs_to_temp() -> (TempDir, ImportPlan) {
    let specs_dir = workspace_root().join("tests/fixtures/.kiro/specs");
    let tmp = tempfile::tempdir().expect("create temp dir");
    let output_dir = tmp.path().join("out");

    let config = config_for(&specs_dir, &output_dir);
    let plan = plan_kiro_import(&config).expect("plan_kiro_import should succeed on real specs");
    (tmp, plan)
}

fn write_and_parse_all_documents(
    plan: &ImportPlan,
) -> (Vec<supersigil_core::SpecDocument>, Vec<String>) {
    let defs = ComponentDefs::defaults();
    let mut docs = Vec::new();
    let mut errors = Vec::new();

    for planned in &plan.documents {
        if let Some(parent) = planned.output_path.parent() {
            std::fs::create_dir_all(parent).expect("create output parent directories");
        }
        std::fs::write(&planned.output_path, &planned.content).expect("write planned spec file");

        match parse_file(&planned.output_path, &defs) {
            Ok(ParseResult::Document(doc)) => docs.push(doc),
            Ok(ParseResult::NotSupersigil(path)) => {
                errors.push(format!("{} parsed as NotSupersigil", path.display()));
            }
            Err(parse_errors) => {
                for err in parse_errors {
                    errors.push(err.to_string());
                }
            }
        }
    }

    (docs, errors)
}

#[test]
fn imported_real_specs_parse_as_supersigil_documents() {
    let (_tmp, plan) = plan_real_specs_to_temp();
    let (_docs, parse_errors) = write_and_parse_all_documents(&plan);

    assert!(
        parse_errors.is_empty(),
        "expected all imported documents to parse; got:\n{}",
        parse_errors.join("\n")
    );
}

#[test]
fn imported_real_specs_build_graph_successfully() {
    let (_tmp, plan) = plan_real_specs_to_temp();
    let (docs, parse_errors) = write_and_parse_all_documents(&plan);

    assert!(
        parse_errors.is_empty(),
        "expected parse step to succeed before graph build; got:\n{}",
        parse_errors.join("\n")
    );

    let graph_config = Config {
        paths: Some(vec!["specs/**/*.md".to_string()]),
        ..Config::default()
    };

    if let Err(errors) = build_graph(docs, &graph_config) {
        let details = errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        panic!("expected graph build success; got errors:\n{details}");
    }
}
