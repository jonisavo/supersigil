use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use supersigil_core::{CodeBlock, ExtractedComponent, Frontmatter, SpanKind, SpecDocument};
use supersigil_evidence::EvidenceKind;

use super::*;
use crate::examples::types::{MatchCheck, MatchFailure};
use crate::test_helpers::{
    build_test_graph, make_acceptance_criteria, make_criterion, make_doc, pos,
};

fn make_example_component(
    id: &str,
    lang: &str,
    runner: &str,
    verifies: Option<&str>,
    code: Option<&str>,
) -> ExtractedComponent {
    let mut attributes = HashMap::from([
        ("id".to_string(), id.to_string()),
        ("lang".to_string(), lang.to_string()),
        ("runner".to_string(), runner.to_string()),
    ]);
    if let Some(v) = verifies {
        attributes.insert("verifies".to_string(), v.to_string());
    }

    let code_blocks = if let Some(c) = code {
        vec![CodeBlock {
            lang: Some(lang.to_string()),
            content: c.to_string(),
            content_offset: 0,
            content_end_offset: c.len(),
            span_kind: SpanKind::RefFence,
        }]
    } else {
        vec![]
    };

    ExtractedComponent {
        name: EXAMPLE.to_string(),
        attributes,
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks,
        position: pos(5),
        end_position: pos(5),
    }
}

fn make_pass_result(spec: ExampleSpec) -> ExampleResult {
    ExampleResult {
        spec,
        outcome: ExampleOutcome::Pass,
        duration: Duration::from_millis(10),
    }
}

fn make_fail_result(spec: ExampleSpec, failures: Vec<MatchFailure>) -> ExampleResult {
    ExampleResult {
        spec,
        outcome: ExampleOutcome::Fail(failures),
        duration: Duration::from_millis(10),
    }
}

fn make_error_result(spec: ExampleSpec, msg: &str) -> ExampleResult {
    ExampleResult {
        spec,
        outcome: ExampleOutcome::Error(msg.to_string()),
        duration: Duration::from_millis(10),
    }
}

fn make_timeout_result(spec: ExampleSpec) -> ExampleResult {
    ExampleResult {
        spec,
        outcome: ExampleOutcome::Timeout,
        duration: Duration::from_millis(30_000),
    }
}

fn make_spec_with_verifies(
    doc_id: &str,
    example_id: &str,
    verifies: Vec<VerifiableRef>,
) -> ExampleSpec {
    ExampleSpec {
        doc_id: doc_id.to_string(),
        example_id: example_id.to_string(),
        lang: "sh".to_string(),
        runner: "sh".to_string(),
        verifies,
        code: "echo hello".to_string(),
        expected: None,
        timeout: 30,
        env: vec![],
        setup: None,
        position: pos(5),
        source_path: PathBuf::from("specs/test.md"),
    }
}

fn build_test_graph_with_components(
    doc_id: &str,
    components: Vec<ExtractedComponent>,
) -> supersigil_core::DocumentGraph {
    let doc = SpecDocument {
        path: PathBuf::from(format!("specs/{doc_id}.md")),
        frontmatter: Frontmatter {
            id: doc_id.to_string(),
            doc_type: None,
            status: None,
        },
        extra: HashMap::new(),
        components,
        warnings: Vec::new(),
    };
    build_test_graph(vec![doc])
}

// -----------------------------------------------------------------------
// 1. collect_examples finds examples in the document graph
// -----------------------------------------------------------------------

#[test]
fn collect_examples_finds_example_in_graph() {
    let example_component = make_example_component(
        "my-example",
        "sh",
        "sh",
        Some("req/auth#crit-1"),
        Some("echo hello"),
    );
    let criterion = make_criterion("crit-1", 2);

    let graph = build_test_graph_with_components(
        "req/auth",
        vec![
            make_acceptance_criteria(vec![criterion], 1),
            example_component,
        ],
    );
    let config = ExamplesConfig::default();

    let specs = collect_examples(&graph, &config);

    assert_eq!(specs.len(), 1, "expected 1 spec, got {}", specs.len());
    let spec = &specs[0];
    assert_eq!(spec.example_id, "my-example");
    assert_eq!(spec.lang, "sh");
    assert_eq!(spec.runner, "sh");
    assert_eq!(spec.code, "echo hello");
    assert_eq!(spec.doc_id, "req/auth");
    assert_eq!(
        spec.verifies,
        vec![VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string()
        }]
    );
}

#[test]
fn collect_examples_derives_lang_from_first_code_block_when_attribute_missing() {
    let mut example_component =
        make_example_component("derived-lang", "txt", "sh", None, Some("echo hello"));
    example_component.attributes.remove("lang");
    example_component.code_blocks[0].lang = Some("bash".to_string());

    let graph = build_test_graph_with_components("req/auth", vec![example_component]);
    let config = ExamplesConfig::default();

    let specs = collect_examples(&graph, &config);

    assert_eq!(specs.len(), 1, "expected 1 spec, got {}", specs.len());
    assert_eq!(specs[0].lang, "bash");
}

#[test]
fn collect_examples_uses_config_default_timeout() {
    let example_component = make_example_component("ex", "sh", "sh", None, None);
    let doc = make_doc("req/doc", vec![example_component]);
    let graph = build_test_graph(vec![doc]);

    let config = ExamplesConfig {
        timeout: 60,
        ..ExamplesConfig::default()
    };

    let specs = collect_examples(&graph, &config);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].timeout, 60);
}

#[test]
fn collect_examples_per_example_timeout_overrides_config() {
    let mut component = make_example_component("ex", "sh", "sh", None, None);
    component
        .attributes
        .insert("timeout".to_string(), "120".to_string());
    let doc = make_doc("req/doc", vec![component]);
    let graph = build_test_graph(vec![doc]);

    let config = ExamplesConfig {
        timeout: 30,
        ..ExamplesConfig::default()
    };

    let specs = collect_examples(&graph, &config);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].timeout, 120);
}

#[test]
fn collect_examples_env_parsed_correctly() {
    let mut component = make_example_component("ex", "sh", "sh", None, None);
    component
        .attributes
        .insert("env".to_string(), "KEY1=val1, KEY2=val2".to_string());
    let doc = make_doc("req/doc", vec![component]);
    let graph = build_test_graph(vec![doc]);
    let config = ExamplesConfig::default();

    let specs = collect_examples(&graph, &config);
    assert_eq!(specs.len(), 1);
    assert_eq!(
        specs[0].env,
        vec![
            ("KEY1".to_string(), "val1".to_string()),
            ("KEY2".to_string(), "val2".to_string()),
        ]
    );
}

#[test]
fn collect_examples_multiple_verifies_refs() {
    let component = make_example_component(
        "ex",
        "sh",
        "sh",
        Some("req/auth#crit-1, req/auth#crit-2"),
        None,
    );
    let doc = make_doc("req/doc", vec![component]);
    // Add the target doc with both criteria so graph ref resolution succeeds.
    let target_doc = make_doc(
        "req/auth",
        vec![make_acceptance_criteria(
            vec![make_criterion("crit-1", 2), make_criterion("crit-2", 3)],
            1,
        )],
    );
    let graph = build_test_graph(vec![doc, target_doc]);
    let config = ExamplesConfig::default();

    let specs = collect_examples(&graph, &config);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].verifies.len(), 2);
    assert!(specs[0].verifies.contains(&VerifiableRef {
        doc_id: "req/auth".to_string(),
        target_id: "crit-1".to_string()
    }));
    assert!(specs[0].verifies.contains(&VerifiableRef {
        doc_id: "req/auth".to_string(),
        target_id: "crit-2".to_string()
    }));
}

#[test]
fn collect_examples_no_examples_returns_empty() {
    let doc = make_doc("req/doc", vec![]);
    let graph = build_test_graph(vec![doc]);
    let config = ExamplesConfig::default();

    let specs = collect_examples(&graph, &config);
    assert!(specs.is_empty());
}

#[test]
fn collect_examples_skips_components_missing_required_attrs() {
    // Component missing 'runner' attribute
    let bad_component = ExtractedComponent {
        name: EXAMPLE.to_string(),
        attributes: HashMap::from([
            ("id".to_string(), "ex".to_string()),
            ("lang".to_string(), "sh".to_string()),
            // no runner
        ]),
        children: vec![],
        body_text: None,
        body_text_offset: None,
        body_text_end_offset: None,
        code_blocks: vec![],
        position: pos(1),
        end_position: pos(1),
    };
    let doc = make_doc("req/doc", vec![bad_component]);
    let graph = build_test_graph(vec![doc]);
    let config = ExamplesConfig::default();

    // Should not panic, just skip
    let specs = collect_examples(&graph, &config);
    assert!(specs.is_empty());
}

// -----------------------------------------------------------------------
// 2. results_to_evidence produces records for passing examples with verifies
// -----------------------------------------------------------------------

#[test]
fn results_to_evidence_pass_with_verifies_produces_record() {
    let spec = make_spec_with_verifies(
        "req/auth",
        "my-example",
        vec![VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        }],
    );
    let result = make_pass_result(spec);
    let evidence = results_to_evidence(&[result]);

    assert_eq!(evidence.len(), 1);
    let rec = &evidence[0];
    assert_eq!(rec.kind(), Some(EvidenceKind::Example));
    assert_eq!(rec.test.name, "my-example");
    assert_eq!(rec.test.file, PathBuf::from("specs/test.md"));
    assert_eq!(rec.test.kind, TestKind::Unknown);

    // Check provenance
    assert_eq!(rec.provenance.len(), 1);
    assert!(matches!(
        &rec.provenance[0],
        PluginProvenance::Example { doc_id, example_id }
        if doc_id == "req/auth" && example_id == "my-example"
    ));

    // Check targets
    let targets: Vec<&VerifiableRef> = rec.targets.iter().collect();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].doc_id, "req/auth");
    assert_eq!(targets[0].target_id, "crit-1");
}

#[test]
fn results_to_evidence_multiple_verifies_produces_one_record_with_all_targets() {
    let spec = make_spec_with_verifies(
        "req/auth",
        "ex",
        vec![
            VerifiableRef {
                doc_id: "req/auth".to_string(),
                target_id: "crit-1".to_string(),
            },
            VerifiableRef {
                doc_id: "req/auth".to_string(),
                target_id: "crit-2".to_string(),
            },
        ],
    );
    let result = make_pass_result(spec);
    let evidence = results_to_evidence(&[result]);

    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].targets.len(), 2);
}

// -----------------------------------------------------------------------
// 3. results_to_evidence skips examples without verifies
// -----------------------------------------------------------------------

#[test]
fn results_to_evidence_skips_pass_without_verifies() {
    let spec = make_spec_with_verifies("req/auth", "no-verifies-example", vec![]);
    let result = make_pass_result(spec);
    let evidence = results_to_evidence(&[result]);

    assert!(
        evidence.is_empty(),
        "passing example without verifies should produce no evidence"
    );
}

#[test]
fn results_to_evidence_skips_failing_examples() {
    let spec = make_spec_with_verifies(
        "req/auth",
        "fail-example",
        vec![VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        }],
    );
    let failure = MatchFailure {
        check: MatchCheck::Body,
        expected: "hello".to_string(),
        actual: "goodbye".to_string(),
    };
    let result = make_fail_result(spec, vec![failure]);
    let evidence = results_to_evidence(&[result]);

    assert!(
        evidence.is_empty(),
        "failing example should produce no evidence"
    );
}

// -----------------------------------------------------------------------
// 4. results_to_findings produces findings for failed examples
// -----------------------------------------------------------------------

#[test]
fn results_to_findings_fail_with_verifies_produces_findings_per_ref() {
    let spec = make_spec_with_verifies(
        "req/auth",
        "fail-example",
        vec![
            VerifiableRef {
                doc_id: "req/auth".to_string(),
                target_id: "crit-1".to_string(),
            },
            VerifiableRef {
                doc_id: "req/auth".to_string(),
                target_id: "crit-2".to_string(),
            },
        ],
    );
    let failure = MatchFailure {
        check: MatchCheck::Body,
        expected: "hello".to_string(),
        actual: "goodbye".to_string(),
    };
    let result = make_fail_result(spec, vec![failure]);
    let findings = results_to_findings(&[result]);

    assert_eq!(findings.len(), 2, "expected 1 finding per verifies ref");
    for f in &findings {
        assert_eq!(f.rule, RuleName::ExampleFailed);
        assert_eq!(f.doc_id, Some("req/auth".to_string()));
        assert!(f.message.contains("fail-example"));
        assert!(f.message.contains("sh")); // runner name
    }

    // Both verifies refs should appear
    let refs: Vec<&str> = findings
        .iter()
        .filter_map(|f| f.details.as_ref())
        .filter_map(|d| d.target_ref.as_deref())
        .collect();
    assert!(refs.contains(&"req/auth#crit-1"));
    assert!(refs.contains(&"req/auth#crit-2"));
}

#[test]
fn results_to_findings_fail_without_verifies_produces_single_finding() {
    let spec = make_spec_with_verifies("req/auth", "fail-no-verifies", vec![]);
    let failure = MatchFailure {
        check: MatchCheck::Status,
        expected: "0".to_string(),
        actual: "1".to_string(),
    };
    let result = make_fail_result(spec, vec![failure]);
    let findings = results_to_findings(&[result]);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::ExampleFailed);
    assert!(
        findings[0]
            .details
            .as_ref()
            .and_then(|d| d.target_ref.as_ref())
            .is_none()
    );
}

#[test]
fn results_to_findings_fail_includes_diff_info() {
    let spec = make_spec_with_verifies("req/auth", "diff-example", vec![]);
    let failure = MatchFailure {
        check: MatchCheck::Body,
        expected: "expected output".to_string(),
        actual: "actual output".to_string(),
    };
    let result = make_fail_result(spec, vec![failure]);
    let findings = results_to_findings(&[result]);

    assert_eq!(findings.len(), 1);
    let details = findings[0].details.as_ref().expect("should have details");
    let code = details.code.as_deref().expect("should have code with diff");
    assert!(
        code.contains("expected output"),
        "diff should include expected: {code}"
    );
    assert!(
        code.contains("actual output"),
        "diff should include actual: {code}"
    );
}

// -----------------------------------------------------------------------
// 5. results_to_findings for errors and timeouts
// -----------------------------------------------------------------------

#[test]
fn results_to_findings_error_produces_finding() {
    let spec = make_spec_with_verifies(
        "req/auth",
        "error-example",
        vec![VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        }],
    );
    let result = make_error_result(spec, "command not found: python");
    let findings = results_to_findings(&[result]);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::ExampleFailed);
    assert!(findings[0].message.contains("error-example"));
    assert!(findings[0].message.contains("command not found: python"));
    assert_eq!(
        findings[0]
            .details
            .as_ref()
            .and_then(|d| d.target_ref.as_deref()),
        Some("req/auth#crit-1")
    );
}

#[test]
fn results_to_findings_error_without_verifies_produces_single_finding() {
    let spec = make_spec_with_verifies("req/auth", "error-no-verifies", vec![]);
    let result = make_error_result(spec, "something went wrong");
    let findings = results_to_findings(&[result]);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::ExampleFailed);
    assert!(findings[0].message.contains("something went wrong"));
}

#[test]
fn results_to_findings_timeout_produces_finding() {
    let spec = make_spec_with_verifies(
        "req/auth",
        "slow-example",
        vec![VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        }],
    );
    let result = make_timeout_result(spec);
    let findings = results_to_findings(&[result]);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].rule, RuleName::ExampleFailed);
    assert!(findings[0].message.contains("slow-example"));
    assert!(
        findings[0].message.contains("timed out"),
        "should mention timeout: {}",
        findings[0].message
    );
}

#[test]
fn results_to_findings_timeout_without_verifies_produces_single_finding() {
    let spec = make_spec_with_verifies("req/auth", "slow-no-verifies", vec![]);
    let result = make_timeout_result(spec);
    let findings = results_to_findings(&[result]);

    assert_eq!(findings.len(), 1);
    assert!(
        findings[0]
            .details
            .as_ref()
            .and_then(|d| d.target_ref.as_ref())
            .is_none()
    );
}

#[test]
fn results_to_findings_pass_produces_no_findings() {
    let spec = make_spec_with_verifies(
        "req/auth",
        "passing-example",
        vec![VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        }],
    );
    let result = make_pass_result(spec);
    let findings = results_to_findings(&[result]);

    assert!(
        findings.is_empty(),
        "passing example should produce no findings"
    );
}

// -----------------------------------------------------------------------
// 6. execute_examples: sequential fallback, parallel, and stable ordering
// -----------------------------------------------------------------------

fn make_sh_spec(doc_id: &str, example_id: &str, code: &str) -> ExampleSpec {
    ExampleSpec {
        doc_id: doc_id.to_string(),
        example_id: example_id.to_string(),
        lang: "sh".to_string(),
        runner: "sh".to_string(),
        verifies: vec![],
        code: code.to_string(),
        expected: None,
        timeout: 30,
        env: vec![],
        setup: None,
        position: pos(1),
        source_path: PathBuf::from("specs/test.md"),
    }
}

#[test]
fn execute_examples_sequential_fallback() {
    // parallelism = 1 → sequential fast path, results in original order
    let specs = vec![
        make_sh_spec("doc/a", "ex-1", "echo one"),
        make_sh_spec("doc/a", "ex-2", "echo two"),
        make_sh_spec("doc/a", "ex-3", "echo three"),
    ];
    let config = ExamplesConfig {
        parallelism: 1,
        ..ExamplesConfig::default()
    };

    let results = execute_examples(&specs, std::path::Path::new("/tmp"), &config);

    assert_eq!(results.len(), 3);
    // All should pass
    for r in &results {
        assert!(
            matches!(r.outcome, ExampleOutcome::Pass),
            "expected Pass for {}, got {:?}",
            r.spec.example_id,
            r.outcome
        );
    }
    // Sorted by (doc_id, example_id)
    assert_eq!(results[0].spec.example_id, "ex-1");
    assert_eq!(results[1].spec.example_id, "ex-2");
    assert_eq!(results[2].spec.example_id, "ex-3");
}

#[test]
fn execute_examples_parallel_produces_same_results() {
    // parallelism > 1 should yield the same set of results as sequential
    let specs = vec![
        make_sh_spec("doc/b", "par-1", "echo alpha"),
        make_sh_spec("doc/b", "par-2", "echo beta"),
        make_sh_spec("doc/b", "par-3", "echo gamma"),
        make_sh_spec("doc/b", "par-4", "echo delta"),
    ];

    let seq_config = ExamplesConfig {
        parallelism: 1,
        ..ExamplesConfig::default()
    };
    let par_config = ExamplesConfig {
        parallelism: 4,
        ..ExamplesConfig::default()
    };
    let root = std::path::Path::new("/tmp");

    let seq_results = execute_examples(&specs, root, &seq_config);
    let par_results = execute_examples(&specs, root, &par_config);

    assert_eq!(seq_results.len(), par_results.len());
    for (seq, par) in seq_results.iter().zip(par_results.iter()) {
        assert_eq!(seq.spec.example_id, par.spec.example_id);
        assert!(
            matches!(seq.outcome, ExampleOutcome::Pass)
                && matches!(par.outcome, ExampleOutcome::Pass),
            "both should pass: seq={:?}, par={:?}",
            seq.outcome,
            par.outcome
        );
    }
}

#[test]
fn execute_examples_stable_ordering() {
    // Results must be sorted by (doc_id, example_id) regardless of
    // execution order.
    let specs = vec![
        make_sh_spec("doc/z", "ex-b", "echo b"),
        make_sh_spec("doc/a", "ex-z", "echo z"),
        make_sh_spec("doc/a", "ex-a", "echo a"),
        make_sh_spec("doc/z", "ex-a", "echo a2"),
    ];
    let config = ExamplesConfig {
        parallelism: 4,
        ..ExamplesConfig::default()
    };

    let results = execute_examples(&specs, std::path::Path::new("/tmp"), &config);

    assert_eq!(results.len(), 4);
    // Expected order: (doc/a, ex-a), (doc/a, ex-z), (doc/z, ex-a), (doc/z, ex-b)
    assert_eq!(results[0].spec.doc_id, "doc/a");
    assert_eq!(results[0].spec.example_id, "ex-a");
    assert_eq!(results[1].spec.doc_id, "doc/a");
    assert_eq!(results[1].spec.example_id, "ex-z");
    assert_eq!(results[2].spec.doc_id, "doc/z");
    assert_eq!(results[2].spec.example_id, "ex-a");
    assert_eq!(results[3].spec.doc_id, "doc/z");
    assert_eq!(results[3].spec.example_id, "ex-b");
}
