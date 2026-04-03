use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::types::{ExpectedSpec, MatchCheck, MatchFailure, MatchFormat};

static UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
        .expect("UUID regex is valid")
});

static ISO8601_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})?)?$")
        .expect("ISO 8601 regex is valid")
});

/// Compare captured output against the expected specification.
///
/// All checks are conjunctive: every applicable check is evaluated and all
/// failures are collected (not short-circuited).
#[must_use]
pub fn match_output(
    actual_output: &str,
    actual_status: Option<u32>,
    expected: &ExpectedSpec,
) -> Vec<MatchFailure> {
    let mut failures = Vec::new();

    // 1. Status check
    if let (Some(exp_status), Some(act_status)) = (expected.status, actual_status)
        && exp_status != act_status
    {
        failures.push(MatchFailure {
            check: MatchCheck::Status,
            expected: exp_status.to_string(),
            actual: act_status.to_string(),
        });
    }

    // 2. Contains check
    if let Some(ref needle) = expected.contains
        && !actual_output.contains(needle.as_str())
    {
        failures.push(MatchFailure {
            check: MatchCheck::Contains,
            expected: needle.clone(),
            actual: actual_output.to_owned(),
        });
    }

    // 3. Body check (per format)
    if let Some(ref body) = expected.body {
        match expected.format {
            MatchFormat::Text => {
                if let Some(failure) = match_text(body, actual_output) {
                    failures.push(failure);
                }
            }
            MatchFormat::Json => {
                failures.extend(match_json(body, actual_output));
            }
            MatchFormat::Regex => {
                if let Some(failure) = match_regex(body, actual_output) {
                    failures.push(failure);
                }
            }
            MatchFormat::Snapshot => {
                // Snapshot matching is handled elsewhere; treat as text for now.
                if let Some(failure) = match_text(body, actual_output) {
                    failures.push(failure);
                }
            }
        }
    }

    failures
}

/// Text matching: trim both sides and compare exactly.
fn match_text(expected: &str, actual: &str) -> Option<MatchFailure> {
    let exp_trimmed = expected.trim();
    let act_trimmed = actual.trim();
    (exp_trimmed != act_trimmed).then(|| MatchFailure {
        check: MatchCheck::Body,
        expected: exp_trimmed.to_owned(),
        actual: act_trimmed.to_owned(),
    })
}

/// JSON matching: deep comparison with wildcard support and path-aware diffs.
fn match_json(expected: &str, actual: &str) -> Vec<MatchFailure> {
    let exp_val: Value = match serde_json::from_str(expected) {
        Ok(v) => v,
        Err(e) => {
            return vec![MatchFailure {
                check: MatchCheck::Body,
                expected: format!("valid JSON: {e}"),
                actual: actual.to_owned(),
            }];
        }
    };

    let act_val: Value = match serde_json::from_str(actual) {
        Ok(v) => v,
        Err(e) => {
            return vec![MatchFailure {
                check: MatchCheck::Body,
                expected: expected.to_owned(),
                actual: format!("invalid JSON: {e}"),
            }];
        }
    };

    let mut diffs = Vec::new();
    compare_json("$", &exp_val, &act_val, &mut diffs);
    diffs
        .into_iter()
        .map(|(path, exp, act)| MatchFailure {
            check: MatchCheck::Body,
            expected: format!("{path}: {exp}"),
            actual: format!("{path}: {act}"),
        })
        .collect()
}

/// Recursively compare two JSON values, collecting path-aware diffs.
fn compare_json(
    path: &str,
    expected: &Value,
    actual: &Value,
    diffs: &mut Vec<(String, String, String)>,
) {
    // Check wildcards first (expected side only)
    if let Value::String(s) = expected {
        match s.as_str() {
            "<any-string>" => {
                if !actual.is_string() {
                    diffs.push((
                        path.to_owned(),
                        "<any-string>".to_owned(),
                        format_value(actual),
                    ));
                }
                return;
            }
            "<any-number>" => {
                if !actual.is_number() {
                    diffs.push((
                        path.to_owned(),
                        "<any-number>".to_owned(),
                        format_value(actual),
                    ));
                }
                return;
            }
            "<any-uuid>" => {
                match actual.as_str() {
                    Some(val) if is_uuid(val) => {}
                    _ => {
                        diffs.push((
                            path.to_owned(),
                            "<any-uuid>".to_owned(),
                            format_value(actual),
                        ));
                    }
                }
                return;
            }
            "<any-iso8601>" => {
                match actual.as_str() {
                    Some(val) if is_iso8601(val) => {}
                    _ => {
                        diffs.push((
                            path.to_owned(),
                            "<any-iso8601>".to_owned(),
                            format_value(actual),
                        ));
                    }
                }
                return;
            }
            _ => {}
        }
    }

    match (expected, actual) {
        (Value::Object(exp_map), Value::Object(act_map)) => {
            // Check for missing keys in actual
            for key in exp_map.keys() {
                if !act_map.contains_key(key) {
                    diffs.push((
                        format!("{path}.{key}"),
                        format_value(&exp_map[key]),
                        "missing".to_owned(),
                    ));
                }
            }
            // Check for extra keys in actual
            for key in act_map.keys() {
                if !exp_map.contains_key(key) {
                    diffs.push((
                        format!("{path}.{key}"),
                        "missing".to_owned(),
                        format_value(&act_map[key]),
                    ));
                }
            }
            // Recurse on common keys
            for (key, exp_v) in exp_map {
                if let Some(act_v) = act_map.get(key) {
                    compare_json(&format!("{path}.{key}"), exp_v, act_v, diffs);
                }
            }
        }
        (Value::Array(exp_arr), Value::Array(act_arr)) => {
            if exp_arr.len() != act_arr.len() {
                diffs.push((
                    path.to_owned(),
                    format!("array of length {}", exp_arr.len()),
                    format!("array of length {}", act_arr.len()),
                ));
                return;
            }
            for (i, (exp_v, act_v)) in exp_arr.iter().zip(act_arr.iter()).enumerate() {
                compare_json(&format!("{path}[{i}]"), exp_v, act_v, diffs);
            }
        }
        _ => {
            if expected != actual {
                diffs.push((
                    path.to_owned(),
                    format_value(expected),
                    format_value(actual),
                ));
            }
        }
    }
}

fn format_value(val: &Value) -> String {
    match val {
        Value::String(s) => format!("\"{s}\""),
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(val).unwrap_or_default(),
    }
}

fn is_uuid(s: &str) -> bool {
    UUID_RE.is_match(s)
}

fn is_iso8601(s: &str) -> bool {
    ISO8601_RE.is_match(s)
}

/// Regex matching: expected body is compiled as a regex and matched against
/// the full actual output.
fn match_regex(pattern: &str, actual: &str) -> Option<MatchFailure> {
    let re = match Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => {
            return Some(MatchFailure {
                check: MatchCheck::Body,
                expected: format!("valid regex: {e}"),
                actual: actual.to_owned(),
            });
        }
    };

    // Anchor the match to the full string: find a match and check it covers
    // the entire input.
    if let Some(m) = re.find(actual)
        && m.start() == 0
        && m.end() == actual.len()
    {
        return None;
    }

    Some(MatchFailure {
        check: MatchCheck::Body,
        expected: pattern.to_owned(),
        actual: actual.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_spec(body: &str) -> ExpectedSpec {
        ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: None,
            body: Some(body.to_owned()),
            body_span: None,
        }
    }

    fn json_spec(body: &str) -> ExpectedSpec {
        ExpectedSpec {
            status: None,
            format: MatchFormat::Json,
            contains: None,
            body: Some(body.to_owned()),
            body_span: None,
        }
    }

    fn regex_spec(body: &str) -> ExpectedSpec {
        ExpectedSpec {
            status: None,
            format: MatchFormat::Regex,
            contains: None,
            body: Some(body.to_owned()),
            body_span: None,
        }
    }

    // -----------------------------------------------------------------------
    // Text matching
    // -----------------------------------------------------------------------

    #[test]
    fn text_exact_match_passes() {
        let spec = text_spec("hello world");
        let failures = match_output("hello world", None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn text_trimmed_whitespace_passes() {
        let spec = text_spec("  hello world  ");
        let failures = match_output("\n hello world \n", None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn text_mismatch_fails_with_body_check() {
        let spec = text_spec("hello");
        let failures = match_output("goodbye", None, &spec);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].check, MatchCheck::Body);
        assert_eq!(failures[0].expected, "hello");
        assert_eq!(failures[0].actual, "goodbye");
    }

    // -----------------------------------------------------------------------
    // JSON matching
    // -----------------------------------------------------------------------

    #[test]
    fn json_exact_object_match() {
        let spec = json_spec(r#"{"name": "Alice", "age": 30}"#);
        let failures = match_output(r#"{"name": "Alice", "age": 30}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_string_matches_any_string() {
        let spec = json_spec(r#"{"name": "<any-string>"}"#);
        let failures = match_output(r#"{"name": "Bob"}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_string_rejects_non_string() {
        let spec = json_spec(r#"{"name": "<any-string>"}"#);
        let failures = match_output(r#"{"name": 42}"#, None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("<any-string>"));
    }

    #[test]
    fn json_any_number_matches_integer() {
        let spec = json_spec(r#"{"count": "<any-number>"}"#);
        let failures = match_output(r#"{"count": 42}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_number_matches_float() {
        let spec = json_spec(r#"{"value": "<any-number>"}"#);
        let failures = match_output(r#"{"value": 3.14}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_number_rejects_string() {
        let spec = json_spec(r#"{"count": "<any-number>"}"#);
        let failures = match_output(r#"{"count": "forty-two"}"#, None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("<any-number>"));
    }

    #[test]
    fn json_any_uuid_matches_valid_uuid() {
        let spec = json_spec(r#"{"id": "<any-uuid>"}"#);
        let failures = match_output(
            r#"{"id": "550e8400-e29b-41d4-a716-446655440000"}"#,
            None,
            &spec,
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_uuid_case_insensitive() {
        let spec = json_spec(r#"{"id": "<any-uuid>"}"#);
        let failures = match_output(
            r#"{"id": "550E8400-E29B-41D4-A716-446655440000"}"#,
            None,
            &spec,
        );
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_uuid_rejects_non_uuid() {
        let spec = json_spec(r#"{"id": "<any-uuid>"}"#);
        let failures = match_output(r#"{"id": "not-a-uuid"}"#, None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("<any-uuid>"));
    }

    #[test]
    fn json_any_uuid_rejects_number() {
        let spec = json_spec(r#"{"id": "<any-uuid>"}"#);
        let failures = match_output(r#"{"id": 42}"#, None, &spec);
        assert_eq!(failures.len(), 1);
    }

    #[test]
    fn json_any_iso8601_matches_datetime() {
        let spec = json_spec(r#"{"created": "<any-iso8601>"}"#);
        let failures = match_output(r#"{"created": "2024-01-15T10:30:00Z"}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_iso8601_matches_date_only() {
        let spec = json_spec(r#"{"created": "<any-iso8601>"}"#);
        let failures = match_output(r#"{"created": "2024-01-15"}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_iso8601_matches_with_offset() {
        let spec = json_spec(r#"{"created": "<any-iso8601>"}"#);
        let failures = match_output(r#"{"created": "2024-01-15T10:30:00+05:30"}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_iso8601_matches_with_millis() {
        let spec = json_spec(r#"{"created": "<any-iso8601>"}"#);
        let failures = match_output(r#"{"created": "2024-01-15T10:30:00.123Z"}"#, None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_any_iso8601_rejects_invalid() {
        let spec = json_spec(r#"{"created": "<any-iso8601>"}"#);
        let failures = match_output(r#"{"created": "not-a-date"}"#, None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("<any-iso8601>"));
    }

    #[test]
    fn json_array_element_by_element() {
        let spec = json_spec(r"[1, 2, 3]");
        let failures = match_output(r"[1, 2, 3]", None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn json_array_length_mismatch() {
        let spec = json_spec(r"[1, 2, 3]");
        let failures = match_output(r"[1, 2]", None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("length 3"));
        assert!(failures[0].actual.contains("length 2"));
    }

    #[test]
    fn json_array_element_mismatch() {
        let spec = json_spec(r"[1, 2, 3]");
        let failures = match_output(r"[1, 99, 3]", None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("$[1]"));
    }

    #[test]
    fn json_path_aware_diff_messages() {
        let spec = json_spec(r#"{"items": [{"name": "Alice"}, {"name": "Bob"}]}"#);
        let failures = match_output(
            r#"{"items": [{"name": "Alice"}, {"name": "Charlie"}]}"#,
            None,
            &spec,
        );
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].expected.contains("$.items[1].name"),
            "expected path $.items[1].name but got: {}",
            failures[0].expected,
        );
    }

    #[test]
    fn json_extra_key_in_actual_fails() {
        let spec = json_spec(r#"{"name": "Alice"}"#);
        let failures = match_output(r#"{"name": "Alice", "extra": true}"#, None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(
            failures[0].expected.contains("$.extra"),
            "should report extra key path, got: {}",
            failures[0].expected,
        );
        assert!(failures[0].expected.contains("missing"));
    }

    #[test]
    fn json_missing_key_in_actual_fails() {
        let spec = json_spec(r#"{"name": "Alice", "age": 30}"#);
        let failures = match_output(r#"{"name": "Alice"}"#, None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].actual.contains("missing"));
    }

    #[test]
    fn json_invalid_expected_json() {
        let spec = json_spec("not json{");
        let failures = match_output(r#"{"name": "Alice"}"#, None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("valid JSON"));
    }

    #[test]
    fn json_invalid_actual_json() {
        let spec = json_spec(r#"{"name": "Alice"}"#);
        let failures = match_output("not json{", None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].actual.contains("invalid JSON"));
    }

    #[test]
    fn json_nested_wildcards() {
        let spec = json_spec(
            r#"{"data": [{"id": "<any-uuid>", "name": "<any-string>", "count": "<any-number>"}]}"#,
        );
        let actual = r#"{"data": [{"id": "550e8400-e29b-41d4-a716-446655440000", "name": "test", "count": 42}]}"#;
        let failures = match_output(actual, None, &spec);
        assert!(failures.is_empty());
    }

    // -----------------------------------------------------------------------
    // Regex matching
    // -----------------------------------------------------------------------

    #[test]
    fn regex_full_match_passes() {
        let spec = regex_spec(r"hello \w+");
        let failures = match_output("hello world", None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn regex_partial_match_fails() {
        let spec = regex_spec(r"hello");
        let failures = match_output("say hello world", None, &spec);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].check, MatchCheck::Body);
    }

    #[test]
    fn regex_multiline_full_match() {
        let spec = regex_spec(r"(?s)line1\nline2.*");
        let failures = match_output("line1\nline2\nline3", None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn regex_invalid_regex_produces_error() {
        let spec = regex_spec(r"[invalid");
        let failures = match_output("anything", None, &spec);
        assert_eq!(failures.len(), 1);
        assert!(failures[0].expected.contains("valid regex"));
    }

    // -----------------------------------------------------------------------
    // Status check
    // -----------------------------------------------------------------------

    #[test]
    fn status_match_passes() {
        let spec = ExpectedSpec {
            status: Some(0),
            format: MatchFormat::Text,
            contains: None,
            body: None,
            body_span: None,
        };
        let failures = match_output("", Some(0), &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn status_mismatch_fails() {
        let spec = ExpectedSpec {
            status: Some(0),
            format: MatchFormat::Text,
            contains: None,
            body: None,
            body_span: None,
        };
        let failures = match_output("", Some(1), &spec);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].check, MatchCheck::Status);
        assert_eq!(failures[0].expected, "0");
        assert_eq!(failures[0].actual, "1");
    }

    #[test]
    fn status_not_checked_when_expected_is_none() {
        let spec = ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: None,
            body: None,
            body_span: None,
        };
        let failures = match_output("", Some(1), &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn status_not_checked_when_actual_is_none() {
        let spec = ExpectedSpec {
            status: Some(0),
            format: MatchFormat::Text,
            contains: None,
            body: None,
            body_span: None,
        };
        let failures = match_output("", None, &spec);
        assert!(failures.is_empty());
    }

    // -----------------------------------------------------------------------
    // Contains check
    // -----------------------------------------------------------------------

    #[test]
    fn contains_match_passes() {
        let spec = ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: Some("world".to_owned()),
            body: None,
            body_span: None,
        };
        let failures = match_output("hello world!", None, &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn contains_mismatch_fails() {
        let spec = ExpectedSpec {
            status: None,
            format: MatchFormat::Text,
            contains: Some("xyz".to_owned()),
            body: None,
            body_span: None,
        };
        let failures = match_output("hello world!", None, &spec);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].check, MatchCheck::Contains);
        assert_eq!(failures[0].expected, "xyz");
    }

    // -----------------------------------------------------------------------
    // Conjunctive checks
    // -----------------------------------------------------------------------

    #[test]
    fn conjunctive_status_ok_body_mismatch_reports_body_failure() {
        let spec = ExpectedSpec {
            status: Some(0),
            format: MatchFormat::Text,
            contains: None,
            body: Some("expected output".to_owned()),
            body_span: None,
        };
        let failures = match_output("actual output", Some(0), &spec);
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].check, MatchCheck::Body);
    }

    #[test]
    fn conjunctive_status_fail_and_body_fail_reports_both() {
        let spec = ExpectedSpec {
            status: Some(0),
            format: MatchFormat::Text,
            contains: None,
            body: Some("expected".to_owned()),
            body_span: None,
        };
        let failures = match_output("actual", Some(1), &spec);
        assert_eq!(failures.len(), 2);
        let checks: Vec<&MatchCheck> = failures.iter().map(|f| &f.check).collect();
        assert!(checks.contains(&&MatchCheck::Status));
        assert!(checks.contains(&&MatchCheck::Body));
    }

    #[test]
    fn conjunctive_all_pass_returns_empty() {
        let spec = ExpectedSpec {
            status: Some(0),
            format: MatchFormat::Text,
            contains: Some("hello".to_owned()),
            body: Some("hello world".to_owned()),
            body_span: None,
        };
        let failures = match_output("hello world", Some(0), &spec);
        assert!(failures.is_empty());
    }

    #[test]
    fn conjunctive_all_three_fail() {
        let spec = ExpectedSpec {
            status: Some(0),
            format: MatchFormat::Text,
            contains: Some("xyz".to_owned()),
            body: Some("expected".to_owned()),
            body_span: None,
        };
        let failures = match_output("actual", Some(1), &spec);
        assert_eq!(failures.len(), 3);
        let checks: Vec<&MatchCheck> = failures.iter().map(|f| &f.check).collect();
        assert!(checks.contains(&&MatchCheck::Status));
        assert!(checks.contains(&&MatchCheck::Contains));
        assert!(checks.contains(&&MatchCheck::Body));
    }

    // -----------------------------------------------------------------------
    // Property tests
    // -----------------------------------------------------------------------

    mod prop {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn any_string_matches_arbitrary_string(s in ".*") {
                let spec = json_spec(r#"{"val": "<any-string>"}"#);
                // Escape the string properly as JSON
                let json_s = serde_json::to_string(&s).unwrap();
                let actual = format!(r#"{{"val": {json_s}}}"#);
                let failures = match_output(&actual, None, &spec);
                prop_assert!(failures.is_empty(), "any-string should match {:?}, failures: {:?}", s, failures);
            }

            #[test]
            fn any_number_matches_arbitrary_i64(n: i64) {
                let spec = json_spec(r#"{"val": "<any-number>"}"#);
                let actual = format!(r#"{{"val": {n}}}"#);
                let failures = match_output(&actual, None, &spec);
                prop_assert!(failures.is_empty(), "any-number should match i64 {n}, failures: {:?}", failures);
            }

            #[test]
            fn any_number_matches_arbitrary_f64(n in proptest::num::f64::NORMAL) {
                let spec = json_spec(r#"{"val": "<any-number>"}"#);
                let actual = format!(r#"{{"val": {n}}}"#);
                let failures = match_output(&actual, None, &spec);
                prop_assert!(failures.is_empty(), "any-number should match f64 {n}, failures: {:?}", failures);
            }
        }
    }
}
