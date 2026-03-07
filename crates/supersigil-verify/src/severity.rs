//! Severity resolution implementing the 4-level precedence chain.

use supersigil_core::VerifyConfig;

use crate::report::{ReportSeverity, RuleName};

/// Resolves the effective severity for a rule, applying the 4-level precedence chain.
///
/// 1. **Draft gating** (highest priority): if `doc_status` is `"draft"`, always
///    return [`ReportSeverity::Info`].
/// 2. **Per-rule override**: if config has a rule-specific severity, use it.
/// 3. **Global strictness**: if config has a global strictness, use it.
/// 4. **Built-in default**: fall through to [`RuleName::default_severity()`].
#[must_use]
pub fn resolve_severity(
    rule: &RuleName,
    doc_status: Option<&str>,
    config: &VerifyConfig,
) -> ReportSeverity {
    // 1. Draft gating (highest priority)
    if doc_status == Some("draft") {
        return ReportSeverity::Info;
    }
    // 2. Per-rule override
    if let Some(sev) = config.rules.get(rule.config_key()) {
        return ReportSeverity::from(*sev);
    }
    // 3. Global strictness
    if let Some(sev) = config.strictness {
        return ReportSeverity::from(sev);
    }
    // 4. Built-in default
    rule.default_severity()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use supersigil_core::Severity;

    use super::*;

    fn empty_config() -> VerifyConfig {
        VerifyConfig {
            strictness: None,
            rules: HashMap::new(),
        }
    }

    #[test]
    fn uses_built_in_default_when_no_overrides() {
        let config = empty_config();
        let result = resolve_severity(&RuleName::UncoveredCriterion, None, &config);
        assert_eq!(result, ReportSeverity::Error);
    }

    #[test]
    fn global_strictness_overrides_default() {
        let config = VerifyConfig {
            strictness: Some(Severity::Warning),
            rules: HashMap::new(),
        };
        let result = resolve_severity(&RuleName::UncoveredCriterion, None, &config);
        assert_eq!(result, ReportSeverity::Warning);
    }

    #[test]
    fn per_rule_override_beats_global_strictness() {
        let config = VerifyConfig {
            strictness: Some(Severity::Warning),
            rules: HashMap::from([("uncovered_criterion".into(), Severity::Error)]),
        };
        let result = resolve_severity(&RuleName::UncoveredCriterion, None, &config);
        assert_eq!(result, ReportSeverity::Error);
    }

    #[test]
    fn draft_gating_beats_everything() {
        let config = VerifyConfig {
            strictness: Some(Severity::Error),
            rules: HashMap::from([("uncovered_criterion".into(), Severity::Error)]),
        };
        let result = resolve_severity(&RuleName::UncoveredCriterion, Some("draft"), &config);
        assert_eq!(result, ReportSeverity::Info);
    }

    #[test]
    fn non_draft_status_uses_normal_resolution() {
        let config = empty_config();
        let result = resolve_severity(&RuleName::UncoveredCriterion, Some("active"), &config);
        assert_eq!(result, ReportSeverity::Error);
    }

    #[test]
    fn off_severity_propagates() {
        let config = VerifyConfig {
            strictness: None,
            rules: HashMap::from([("uncovered_criterion".into(), Severity::Off)]),
        };
        let result = resolve_severity(&RuleName::UncoveredCriterion, None, &config);
        assert_eq!(result, ReportSeverity::Off);
    }

    mod prop {
        use std::collections::HashMap;

        use proptest::prelude::*;
        use supersigil_core::Severity;

        use super::super::*;

        fn arb_severity() -> impl Strategy<Value = Severity> {
            prop_oneof![
                Just(Severity::Off),
                Just(Severity::Warning),
                Just(Severity::Error),
            ]
        }

        fn arb_rule_name() -> impl Strategy<Value = RuleName> {
            (0..RuleName::ALL.len()).prop_map(|i| RuleName::ALL[i])
        }

        proptest! {
            #[test]
            fn draft_always_produces_info(
                rule in arb_rule_name(),
                strictness in proptest::option::of(arb_severity()),
                per_rule in proptest::option::of(arb_severity()),
            ) {
                let mut rules = HashMap::new();
                if let Some(sev) = per_rule {
                    rules.insert(rule.config_key().to_owned(), sev);
                }
                let config = VerifyConfig { strictness, rules };
                let result = resolve_severity(&rule, Some("draft"), &config);
                prop_assert_eq!(result, ReportSeverity::Info);
            }

            #[test]
            fn per_rule_beats_global(
                rule in arb_rule_name(),
                global in arb_severity(),
                per_rule_sev in arb_severity(),
            ) {
                let config = VerifyConfig {
                    strictness: Some(global),
                    rules: HashMap::from([(rule.config_key().to_owned(), per_rule_sev)]),
                };
                let result = resolve_severity(&rule, Some("active"), &config);
                prop_assert_eq!(result, ReportSeverity::from(per_rule_sev));
            }
        }
    }
}
