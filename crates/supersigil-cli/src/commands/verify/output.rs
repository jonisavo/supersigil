use supersigil_verify::{ExampleOutcome, ExampleResult};

mod hints;
mod progress;
mod snapshot;
mod terminal;

pub(super) use hints::{count_example_pending_criteria, remediation_hints};
pub(super) use progress::{
    ExampleProgressDisplay, ExampleProgressReporter, should_render_example_summary,
};
#[cfg(test)]
pub(super) use progress::{
    ExampleProgressEntry, ExampleProgressSnapshot, ExampleProgressState, render_progress_snapshot,
};
pub(super) use snapshot::update_snapshots;
pub(super) use terminal::format_terminal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExampleExecutionSummary {
    pub(super) passed: usize,
    pub(super) failures: Vec<ExampleFailure>,
}

impl ExampleExecutionSummary {
    pub(super) fn failed(&self) -> usize {
        self.failures.len()
    }

    pub(super) fn from_results(results: &[ExampleResult]) -> Self {
        let mut passed = 0;
        let mut failures = Vec::new();

        for result in results {
            match &result.outcome {
                ExampleOutcome::Pass => passed += 1,
                ExampleOutcome::Fail(match_failures) => failures.push(ExampleFailure {
                    doc_id: result.spec.doc_id.clone(),
                    example_id: result.spec.example_id.clone(),
                    runner: result.spec.runner.clone(),
                    details: if match_failures.is_empty() {
                        vec![ExampleFailureDetail::Message(
                            "output did not match expected result".to_string(),
                        )]
                    } else {
                        match_failures
                            .iter()
                            .map(|failure| ExampleFailureDetail::Match {
                                check: format!("{:?}", failure.check),
                                expected: failure.expected.clone(),
                                actual: failure.actual.clone(),
                            })
                            .collect()
                    },
                }),
                ExampleOutcome::Timeout => failures.push(ExampleFailure {
                    doc_id: result.spec.doc_id.clone(),
                    example_id: result.spec.example_id.clone(),
                    runner: result.spec.runner.clone(),
                    details: vec![ExampleFailureDetail::Message(format!(
                        "timed out after {}s",
                        result.spec.timeout
                    ))],
                }),
                ExampleOutcome::Error(error) => failures.push(ExampleFailure {
                    doc_id: result.spec.doc_id.clone(),
                    example_id: result.spec.example_id.clone(),
                    runner: result.spec.runner.clone(),
                    details: vec![ExampleFailureDetail::Message(error.clone())],
                }),
            }
        }

        Self { passed, failures }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExampleFailure {
    pub(super) doc_id: String,
    pub(super) example_id: String,
    pub(super) runner: String,
    pub(super) details: Vec<ExampleFailureDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ExampleFailureDetail {
    Match {
        check: String,
        expected: String,
        actual: String,
    },
    Message(String),
}

fn pluralize(count: usize, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{singular}s")
    }
}
