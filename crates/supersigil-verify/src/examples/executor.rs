//! Executor: orchestrates runner dispatch, output matching, and evidence/finding production.
//!
//! Provides four public functions:
//! - [`collect_examples`]: walk the document graph and build `ExampleSpec`s.
//! - [`execute_examples`]: run a list of specs and return `ExampleResult`s.
//! - [`results_to_evidence`]: convert passing results (with verifies) to evidence records.
//! - [`results_to_findings`]: convert failing/erroring/timeout results to findings.

use std::collections::BTreeMap;
use std::path::Path;

use supersigil_core::{DocumentGraph, EXAMPLE, EXPECTED, ExamplesConfig};
use supersigil_evidence::{
    EvidenceId, PluginProvenance, SourceLocation, TestIdentity, TestKind, VerifiableRef,
    VerificationEvidenceRecord, VerificationTargets,
};

use super::runner;
use super::types::{ExampleOutcome, ExampleResult, ExampleSpec, ExpectedSpec, MatchFormat};
use crate::report::{Finding, FindingDetails, RuleName};

/// Optional observer for live example execution progress.
pub trait ExampleProgressObserver: Send + Sync {
    /// Called immediately before an example starts running.
    fn example_started(&self, _spec: &ExampleSpec) {}

    /// Called after an example finishes and its outcome is known.
    fn example_finished(&self, _result: &ExampleResult) {}
}

// ---------------------------------------------------------------------------
// collect_examples
// ---------------------------------------------------------------------------

/// Walk the document graph and collect all `ExampleSpec`s.
///
/// For each document, recursively searches for `Example` components and
/// extracts their configuration. Missing required attributes are silently
/// skipped to avoid hard failures in non-example documents.
#[must_use]
pub fn collect_examples(graph: &DocumentGraph, config: &ExamplesConfig) -> Vec<ExampleSpec> {
    let mut specs = Vec::new();

    for (doc_id, doc) in graph.documents() {
        for component in &doc.components {
            collect_from_component(component, doc_id, doc, config, &mut specs);
        }
    }

    specs
}

/// Recursively collect `Example` components from a component tree.
fn collect_from_component(
    component: &supersigil_core::ExtractedComponent,
    doc_id: &str,
    doc: &supersigil_core::SpecDocument,
    config: &ExamplesConfig,
    specs: &mut Vec<ExampleSpec>,
) {
    if component.name == EXAMPLE
        && let Some(spec) = build_example_spec(component, doc_id, doc, config)
    {
        specs.push(spec);
    }

    // Recurse into children regardless of the component name, so we find
    // nested Example components.
    for child in &component.children {
        collect_from_component(child, doc_id, doc, config, specs);
    }
}

/// Build an `ExampleSpec` from an `Example` component.
///
/// Returns `None` if required attributes are missing or if no language can be
/// determined from either the `lang` attribute or the first fenced code block.
fn build_example_spec(
    component: &supersigil_core::ExtractedComponent,
    doc_id: &str,
    doc: &supersigil_core::SpecDocument,
    config: &ExamplesConfig,
) -> Option<ExampleSpec> {
    let example_id = component.attributes.get("id")?.clone();
    let lang = component.attributes.get("lang").cloned().or_else(|| {
        component
            .code_blocks
            .first()
            .and_then(|block| block.lang.clone())
    })?;
    let runner_name = component.attributes.get("runner")?.clone();

    // Extract verifies: parse comma-separated refs like "doc#crit-1, doc#crit-2"
    let verifies = component
        .attributes
        .get("verifies")
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter_map(VerifiableRef::parse)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Extract code from first code block
    let code = component
        .code_blocks
        .first()
        .map(|cb| cb.content.clone())
        .unwrap_or_default();

    // Extract timeout (per-example override or config default)
    let timeout = component
        .attributes
        .get("timeout")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(config.timeout);

    // Extract env: comma-separated "KEY=VALUE" pairs
    let env = component
        .attributes
        .get("env")
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter_map(|pair| {
                    let (key, value) = pair.split_once('=')?;
                    Some((key.trim().to_string(), value.trim().to_string()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Extract setup path
    let setup = component
        .attributes
        .get("setup")
        .map(|s| std::path::PathBuf::from(s.trim()));

    // Look for Expected child component
    let expected = component
        .children
        .iter()
        .find(|c| c.name == EXPECTED)
        .map(build_expected_spec);

    Some(ExampleSpec {
        doc_id: doc_id.to_string(),
        example_id,
        lang,
        runner: runner_name,
        verifies,
        code,
        expected,
        timeout,
        env,
        setup,
        position: component.position,
        source_path: doc.path.clone(),
    })
}

/// Build an `ExpectedSpec` from an `Expected` child component.
fn build_expected_spec(component: &supersigil_core::ExtractedComponent) -> ExpectedSpec {
    let status = component
        .attributes
        .get("status")
        .and_then(|s| s.parse::<u32>().ok());

    let format = component
        .attributes
        .get("format")
        .map_or(MatchFormat::Text, |f| match f.trim() {
            "json" => MatchFormat::Json,
            "regex" => MatchFormat::Regex,
            "snapshot" => MatchFormat::Snapshot,
            _ => MatchFormat::Text,
        });

    let contains = component.attributes.get("contains").cloned();

    // Extract body from first code block content
    let body = component.code_blocks.first().map(|cb| cb.content.clone());

    // Extract body_span as (content_offset, content_offset + content.len())
    let body_span = component.code_blocks.first().map(|cb| {
        let start = cb.content_offset;
        let end = start + cb.content.len();
        (start, end)
    });

    ExpectedSpec {
        status,
        format,
        contains,
        body,
        body_span,
    }
}

// ---------------------------------------------------------------------------
// execute_examples
// ---------------------------------------------------------------------------

/// Execute a list of example specs and return results.
///
/// When `config.parallelism > 1` and there are multiple specs, examples are
/// run concurrently using a pool of worker threads. Results are always
/// returned sorted by `(doc_id, example_id)` for stable report ordering.
///
/// # Panics
///
/// Panics if the internal results mutex is poisoned, which can only happen if
/// a worker thread panics while holding the lock — an unrecoverable error.
#[must_use]
pub fn execute_examples(
    specs: &[ExampleSpec],
    project_root: &Path,
    config: &ExamplesConfig,
) -> Vec<ExampleResult> {
    execute_examples_with_progress(specs, project_root, config, None)
}

/// Execute a list of example specs and report live progress to an optional observer.
///
/// # Panics
///
/// Panics if the internal results mutex is poisoned, which can only happen if
/// a worker thread panics while holding the lock — an unrecoverable error.
#[must_use]
pub fn execute_examples_with_progress(
    specs: &[ExampleSpec],
    project_root: &Path,
    config: &ExamplesConfig,
    observer: Option<&dyn ExampleProgressObserver>,
) -> Vec<ExampleResult> {
    let parallelism = config.parallelism.max(1);

    let mut results = if parallelism == 1 || specs.len() <= 1 {
        // Fast path: no concurrency needed, run sequentially.
        specs
            .iter()
            .map(|spec| {
                if let Some(observer) = observer {
                    observer.example_started(spec);
                }
                let result = runner::run_example(spec, project_root, config);
                if let Some(observer) = observer {
                    observer.example_finished(&result);
                }
                result
            })
            .collect::<Vec<_>>()
    } else {
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Lock-free work counter: each worker atomically claims the next index.
        let next_idx = AtomicUsize::new(0);
        let results: Mutex<Vec<(usize, ExampleResult)>> =
            Mutex::new(Vec::with_capacity(specs.len()));

        std::thread::scope(|s| {
            for _ in 0..parallelism {
                s.spawn(|| {
                    loop {
                        let idx = next_idx.fetch_add(1, Ordering::Relaxed);
                        if idx >= specs.len() {
                            return;
                        }
                        if let Some(observer) = observer {
                            observer.example_started(&specs[idx]);
                        }
                        let result = runner::run_example(&specs[idx], project_root, config);
                        if let Some(observer) = observer {
                            observer.example_finished(&result);
                        }
                        results.lock().unwrap().push((idx, result));
                    }
                });
            }
        });

        // Sort by original index so that the subsequent stable sort by
        // (doc_id, example_id) has a consistent tie-breaker.
        let mut indexed = results.into_inner().unwrap();
        indexed.sort_by_key(|(idx, _)| *idx);
        indexed.into_iter().map(|(_, r)| r).collect()
    };

    // Stable sort by (doc_id, example_id) for deterministic report ordering.
    results.sort_by(|a, b| {
        a.spec
            .doc_id
            .cmp(&b.spec.doc_id)
            .then(a.spec.example_id.cmp(&b.spec.example_id))
    });

    results
}

// ---------------------------------------------------------------------------
// results_to_evidence
// ---------------------------------------------------------------------------

/// Convert passing example results with verifies into evidence records.
///
/// Only results with `ExampleOutcome::Pass` AND non-empty `verifies` produce
/// evidence records. Results without verifies (informational examples) are
/// skipped.
#[must_use]
pub fn results_to_evidence(results: &[ExampleResult]) -> Vec<VerificationEvidenceRecord> {
    let mut records = Vec::new();

    for (idx, result) in results.iter().enumerate() {
        // Only passing results with verifies targets produce evidence
        if !matches!(result.outcome, ExampleOutcome::Pass) {
            continue;
        }

        let verifies = &result.spec.verifies;
        if verifies.is_empty() {
            continue;
        }

        let targets_set: std::collections::BTreeSet<VerifiableRef> =
            verifies.iter().cloned().collect();

        let Some(targets) = VerificationTargets::new(targets_set) else {
            continue;
        };

        let spec = &result.spec;

        let record = VerificationEvidenceRecord {
            id: EvidenceId::new(idx),
            targets,
            test: TestIdentity {
                file: spec.source_path.clone(),
                name: spec.example_id.clone(),
                kind: TestKind::Unknown,
            },
            source_location: SourceLocation {
                file: spec.source_path.clone(),
                line: spec.position.line,
                column: spec.position.column,
            },
            provenance: vec![PluginProvenance::Example {
                doc_id: spec.doc_id.clone(),
                example_id: spec.example_id.clone(),
            }],
            metadata: BTreeMap::new(),
        };

        records.push(record);
    }

    records
}

// ---------------------------------------------------------------------------
// results_to_findings
// ---------------------------------------------------------------------------

/// Convert failing, erroring, or timed-out example results into findings.
///
/// - `ExampleOutcome::Fail(failures)`: produces one finding per verifies ref
///   (or one finding without a ref if verifies is empty), with diff details.
/// - `ExampleOutcome::Error(msg)`: produces a finding with the error message.
/// - `ExampleOutcome::Timeout`: produces a finding noting the timeout.
/// - `ExampleOutcome::Pass`: skipped.
#[must_use]
pub fn results_to_findings(results: &[ExampleResult]) -> Vec<Finding> {
    let mut findings = Vec::new();

    for result in results {
        let spec = &result.spec;

        let (message, code) = match &result.outcome {
            ExampleOutcome::Pass => continue,
            ExampleOutcome::Fail(failures) => {
                let diff_summary = if failures.is_empty() {
                    "example failed".to_string()
                } else {
                    failures
                        .iter()
                        .map(|f| {
                            format!(
                                "[{:?}] expected {:?}, got {:?}",
                                f.check, f.expected, f.actual
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                let msg = format!(
                    "example '{}' (runner: {}) failed: {}",
                    spec.example_id, spec.runner, diff_summary
                );
                (msg, Some(diff_summary))
            }
            ExampleOutcome::Error(err) => {
                let msg = format!(
                    "example '{}' (runner: {}) error: {}",
                    spec.example_id, spec.runner, err
                );
                (msg, None)
            }
            ExampleOutcome::Timeout => {
                let msg = format!(
                    "example '{}' (runner: {}) timed out after {}s",
                    spec.example_id, spec.runner, spec.timeout
                );
                (msg, None)
            }
        };

        emit_findings_for_outcome(spec, &message, code.as_deref(), &mut findings);
    }

    findings
}

/// Emit one finding per verifies ref (or a single finding if verifies is empty).
fn emit_findings_for_outcome(
    spec: &ExampleSpec,
    message: &str,
    code: Option<&str>,
    findings: &mut Vec<Finding>,
) {
    let path = spec.source_path.to_string_lossy().into_owned();

    if spec.verifies.is_empty() {
        let finding = Finding::new(
            RuleName::ExampleFailed,
            Some(spec.doc_id.clone()),
            message.to_string(),
            Some(spec.position),
        )
        .with_details(FindingDetails {
            path: Some(path),
            code: code.map(ToString::to_string),
            ..FindingDetails::default()
        });
        findings.push(finding);
    } else {
        for verifiable_ref in &spec.verifies {
            let finding = Finding::new(
                RuleName::ExampleFailed,
                Some(spec.doc_id.clone()),
                message.to_string(),
                Some(spec.position),
            )
            .with_details(FindingDetails {
                path: Some(path.clone()),
                target_ref: Some(verifiable_ref.to_string()),
                code: code.map(ToString::to_string),
                ..FindingDetails::default()
            });
            findings.push(finding);
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use supersigil_core::{CodeBlock, ExtractedComponent, Frontmatter, SpecDocument};
    use supersigil_evidence::EvidenceKind;

    use super::*;
    use crate::examples::types::{MatchCheck, MatchFailure};
    use crate::test_helpers::{make_doc, pos};

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
            }]
        } else {
            vec![]
        };

        ExtractedComponent {
            name: EXAMPLE.to_string(),
            attributes,
            children: vec![],
            body_text: None,
            code_blocks,
            position: pos(5),
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
            source_path: PathBuf::from("specs/test.mdx"),
        }
    }

    fn build_test_graph_with_components(
        doc_id: &str,
        components: Vec<ExtractedComponent>,
    ) -> supersigil_core::DocumentGraph {
        let doc = SpecDocument {
            path: PathBuf::from(format!("specs/{doc_id}.mdx")),
            frontmatter: Frontmatter {
                id: doc_id.to_string(),
                doc_type: None,
                status: None,
            },
            extra: HashMap::new(),
            components,
        };
        crate::test_helpers::build_test_graph(vec![doc])
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

        let graph = build_test_graph_with_components("req/auth", vec![example_component]);
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
        let graph = crate::test_helpers::build_test_graph(vec![doc]);

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
        let graph = crate::test_helpers::build_test_graph(vec![doc]);

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
        let graph = crate::test_helpers::build_test_graph(vec![doc]);
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
        let graph = crate::test_helpers::build_test_graph(vec![doc]);
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
        let graph = crate::test_helpers::build_test_graph(vec![doc]);
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
            code_blocks: vec![],
            position: pos(1),
        };
        let doc = make_doc("req/doc", vec![bad_component]);
        let graph = crate::test_helpers::build_test_graph(vec![doc]);
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
        assert_eq!(rec.test.file, PathBuf::from("specs/test.mdx"));
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
            source_path: PathBuf::from("specs/test.mdx"),
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
}
