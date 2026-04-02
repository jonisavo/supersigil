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
use super::types::{
    BodySpan, ExampleOutcome, ExampleResult, ExampleSpec, ExpectedSpec, MatchFormat,
};
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

    // Extract body_span from the code block's raw source offsets.
    // Using `content_end_offset` (instead of computing from decoded content
    // length) is critical for inline XML body text with entity references
    // like `&lt;` — the decoded content is shorter than the raw source.
    let body_span = component.code_blocks.first().map(|cb| BodySpan {
        start: cb.content_offset,
        end: cb.content_end_offset,
        kind: cb.span_kind,
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
mod tests;
