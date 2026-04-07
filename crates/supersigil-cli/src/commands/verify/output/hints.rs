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
        let suggestion = finding
            .details
            .as_ref()
            .and_then(|details| details.suggestion.as_ref());

        match finding.rule {
            RuleName::PluginDiscoveryFailure => {
                if let Some(suggestion) = suggestion
                    && !hints.iter().any(|hint| hint == suggestion)
                {
                    hints.push(suggestion.clone());
                }
            }
            RuleName::IncompleteDecision => {
                push_unique(
                    &mut hints,
                    "Add a `<Rationale>` child inside each `<Decision>` to explain why it was chosen.",
                );
            }
            RuleName::OrphanDecision => {
                push_unique(
                    &mut hints,
                    "Link orphan decisions with `<References>` or reference them from another document.",
                );
            }
            RuleName::InvalidVerifiedByPlacement => {
                push_unique(
                    &mut hints,
                    "`<VerifiedBy>` must be a direct child of a `<Criterion>` component.",
                );
            }
            RuleName::InvalidRationalePlacement => {
                push_unique(
                    &mut hints,
                    "`<Rationale>` must be a direct child of a `<Decision>` component.",
                );
            }
            RuleName::InvalidAlternativePlacement => {
                push_unique(
                    &mut hints,
                    "`<Alternative>` must be a direct child of a `<Decision>` component.",
                );
            }
            RuleName::InvalidAlternativeStatus => {
                push_unique(
                    &mut hints,
                    "Valid `<Alternative>` status values: rejected, deferred, superseded.",
                );
            }
            RuleName::StatusInconsistency => {
                push_unique(
                    &mut hints,
                    "Update the document `status` in the front matter to reflect current progress.",
                );
            }
            _ => {}
        }
    }

    hints
}

fn push_unique(hints: &mut Vec<String>, hint: &str) {
    if !hints.iter().any(|h| h == hint) {
        hints.push(hint.to_string());
    }
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
