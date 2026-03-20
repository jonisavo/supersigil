use std::collections::HashSet;

use supersigil_core::{ComponentDefs, Config, DocumentGraph, VERIFIED_BY};
use supersigil_verify::{RuleName, VerificationReport};

pub(crate) fn remediation_hints(
    report: &VerificationReport,
    config: &Config,
    graph: &DocumentGraph,
) -> Vec<String> {
    let mut hints = Vec::new();

    if report
        .findings
        .iter()
        .any(|finding| finding.rule == RuleName::MissingVerificationEvidence)
    {
        hints.push(
            "Run `supersigil refs` to list canonical criterion refs you can copy into evidence."
                .to_string(),
        );

        if config
            .ecosystem
            .plugins
            .iter()
            .any(|plugin| plugin.as_str() == "rust")
        {
            hints.push(
                "Rust-native fix: annotate a supported test with `#[verifies(\"doc#criterion\")]`."
                    .to_string(),
            );
        }

        hints.push(authored_evidence_hint(graph.component_defs()));
    }

    for finding in &report.findings {
        if finding.rule != RuleName::PluginDiscoveryFailure {
            continue;
        }
        let Some(suggestion) = finding
            .details
            .as_ref()
            .and_then(|details| details.suggestion.as_ref())
        else {
            continue;
        };
        if !hints.iter().any(|hint| hint == suggestion) {
            hints.push(suggestion.clone());
        }
    }

    hints
}

fn authored_evidence_hint(defs: &ComponentDefs) -> String {
    let Some(examples) = defs.get(VERIFIED_BY).map(|def| def.examples.as_slice()) else {
        return "Authored fix: add criterion-nested `<VerifiedBy ... />` evidence.".to_string();
    };

    let quoted_examples: Vec<String> = examples
        .iter()
        .take(2)
        .map(|example| format!("`{example}`"))
        .collect();

    match quoted_examples.as_slice() {
        [] => "Authored fix: add criterion-nested `<VerifiedBy ... />` evidence.".to_string(),
        [example] => format!("Authored fix: add criterion-nested {example} evidence."),
        [first, second] => {
            format!("Authored fix: add criterion-nested {first} or {second} evidence.")
        }
        _ => unreachable!("only the first two examples are used"),
    }
}

/// Count how many `MissingVerificationEvidence` findings target criteria that
/// have `<Example verifies="...">` refs, i.e. criteria that would be covered
/// if examples had been executed.
pub(crate) fn count_example_pending_criteria(
    report: &VerificationReport,
    graph: &DocumentGraph,
) -> usize {
    let example_refs = crate::scope::collect_example_verifies_refs(graph);
    if example_refs.is_empty() {
        return 0;
    }

    report
        .findings
        .iter()
        .filter_map(|finding| {
            (finding.rule == RuleName::MissingVerificationEvidence)
                .then_some(finding)
                .and_then(|finding| finding.details.as_ref())
                .and_then(|details| details.target_ref.as_deref())
                .filter(|target_ref| example_refs.contains(*target_ref))
        })
        .collect::<HashSet<_>>()
        .len()
}
