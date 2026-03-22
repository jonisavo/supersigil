use std::path::PathBuf;
use std::time::Duration;

use supersigil_core::SourcePosition;
use supersigil_verify::{ExampleOutcome, ExampleResult, ExampleSpec, MatchCheck, MatchFailure};

#[test]
fn example_failures_are_constructible_from_public_api() {
    let result = ExampleResult {
        spec: ExampleSpec {
            doc_id: "req/auth".into(),
            example_id: "login".into(),
            lang: "rust".into(),
            runner: "cargo-test".into(),
            verifies: Vec::new(),
            code: "assert!(true);".into(),
            expected: None,
            timeout: 30,
            env: Vec::new(),
            setup: Some(PathBuf::from("tests/setup.sh")),
            position: SourcePosition {
                byte_offset: 0,
                line: 1,
                column: 1,
            },
            source_path: PathBuf::from("specs/req/auth.md"),
        },
        outcome: ExampleOutcome::Fail(vec![MatchFailure {
            check: MatchCheck::Body,
            expected: "expected".into(),
            actual: "actual".into(),
        }]),
        duration: Duration::from_secs(1),
    };

    assert!(matches!(result.outcome, ExampleOutcome::Fail(_)));
}
