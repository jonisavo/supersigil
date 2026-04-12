//! File discovery and `verifies()` extraction for JS/TS test files.
//!
//! Filters the shared project-level test files to JS/TS extensions and
//! parses each with `oxc` to extract `verifies()` call expressions from
//! `test()` / `it()` calls.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, ArrayExpression, CallExpression, Expression, ObjectPropertyKind, PropertyKey,
    Statement,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use supersigil_core::DocumentGraph;
use supersigil_evidence::{
    EcosystemPlugin, EvidenceId, PluginDiagnostic, PluginDiscoveryResult, PluginError,
    PluginErrorDetails, PluginProvenance, ProjectScope, SourceLocation, TestIdentity, TestKind,
    VerifiableRef, VerificationEvidenceRecord, VerificationTargets, WorkspaceMetadata,
};

const PLUGIN_NAME: &str = "js";

// ---------------------------------------------------------------------------
// JsPlugin
// ---------------------------------------------------------------------------

/// Built-in JS/TS ecosystem plugin.
///
/// Discovers `verifies()` evidence by parsing JavaScript and TypeScript
/// test files with `oxc`.
#[derive(Debug)]
pub struct JsPlugin;

const JS_TS_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];

/// Check whether a path has a JS/TS file extension.
fn is_js_ts_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| JS_TS_EXTENSIONS.contains(&ext))
}

// ---------------------------------------------------------------------------
// EcosystemPlugin implementation
// ---------------------------------------------------------------------------

impl EcosystemPlugin for JsPlugin {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn plan_discovery_inputs<'a>(
        &self,
        test_files: &'a [PathBuf],
        _scope: &ProjectScope,
    ) -> Cow<'a, [PathBuf]> {
        let js_files: Vec<PathBuf> = test_files
            .iter()
            .filter(|p| is_js_ts_file(p))
            .cloned()
            .collect();
        Cow::Owned(js_files)
    }

    fn workspace_metadata(&self, _workspace_root: &Path) -> Result<WorkspaceMetadata, PluginError> {
        Ok(WorkspaceMetadata { repository: None })
    }

    #[allow(
        clippy::too_many_lines,
        reason = "sequential AST extraction with inline diagnostics"
    )]
    fn discover(
        &self,
        files: &[PathBuf],
        _scope: &ProjectScope,
        _documents: &DocumentGraph,
    ) -> Result<PluginDiscoveryResult, PluginError> {
        let mut result = PluginDiscoveryResult::default();
        let mut next_id: usize = 0;

        for file in files {
            let source_text = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(err) => {
                    result.diagnostics.push(PluginDiagnostic::warning_for_path(
                        file.clone(),
                        format!("skipping due to I/O error: {err}"),
                    ));
                    continue;
                }
            };

            if !source_text.contains("verifies") {
                continue;
            }

            let source_type = SourceType::from_path(file).unwrap_or_default();
            let allocator = Allocator::default();
            let ret = Parser::new(&allocator, &source_text, source_type).parse();

            if ret.panicked {
                let message = if ret.errors.is_empty() {
                    "unrecoverable parse error".to_string()
                } else {
                    ret.errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                result.diagnostics.push(PluginDiagnostic::warning_for_path(
                    file.clone(),
                    format!("skipping due to parse error: {message}"),
                ));
                continue;
            }

            // Recoverable parse errors: emit diagnostics but still process the AST.
            if !ret.errors.is_empty() {
                let message = ret
                    .errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                result.diagnostics.push(PluginDiagnostic::warning_for_path(
                    file.clone(),
                    format!("recoverable parse errors (AST still processed): {message}"),
                ));
            }

            let mut describe_stack: Vec<String> = Vec::new();
            let mut ctx = WalkCtx {
                file,
                source: &source_text,
                result: &mut result,
                next_id: &mut next_id,
            };
            walk_statements(&ret.program.body, &mut describe_stack, &mut ctx)?;
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Recursive statement walker
// ---------------------------------------------------------------------------

struct WalkCtx<'a> {
    file: &'a Path,
    source: &'a str,
    result: &'a mut PluginDiscoveryResult,
    next_id: &'a mut usize,
}

fn walk_statements(
    stmts: &[Statement<'_>],
    describe_stack: &mut Vec<String>,
    ctx: &mut WalkCtx<'_>,
) -> Result<(), PluginError> {
    for stmt in stmts {
        let call = match stmt {
            Statement::ExpressionStatement(es) => match &es.expression {
                Expression::CallExpression(c) => c,
                _ => continue,
            },
            _ => continue,
        };

        let callee_name = match &call.callee {
            Expression::Identifier(id) => &*id.name,
            _ => continue,
        };

        if callee_name == "describe" {
            let suite_name = match call.arguments.first() {
                Some(Argument::StringLiteral(s)) => s.value.to_string(),
                _ => continue,
            };

            let body_stmts = match call.arguments.get(1) {
                Some(Argument::ArrowFunctionExpression(arrow)) => &arrow.body.statements,
                Some(Argument::FunctionExpression(func)) => match &func.body {
                    Some(body) => &body.statements,
                    None => continue,
                },
                _ => continue,
            };

            describe_stack.push(suite_name);
            walk_statements(body_stmts, describe_stack, ctx)?;
            describe_stack.pop();
            continue;
        }

        if callee_name != "test" && callee_name != "it" {
            continue;
        }

        let raw_test_name = match call.arguments.first() {
            Some(Argument::StringLiteral(s)) => s.value.to_string(),
            _ => continue,
        };

        let test_name = if describe_stack.is_empty() {
            raw_test_name
        } else {
            let mut parts: Vec<&str> = describe_stack.iter().map(String::as_str).collect();
            parts.push(&raw_test_name);
            parts.join(" > ")
        };

        let verifies_call = find_verifies_call(&call.arguments[1..]);
        let (extraction, span_start) = if let Some(verifies) = verifies_call {
            (
                extract_verifies_refs(verifies, ctx.file)?,
                verifies.span.start,
            )
        } else if let Some((array, span)) = find_raw_meta_verifies(&call.arguments[1..]) {
            (extract_array_refs(array, ctx.file)?, span)
        } else {
            continue;
        };

        ctx.result.diagnostics.extend(extraction.diagnostics);

        if extraction.refs.is_empty() {
            if extraction.had_non_string_args {
                ctx.result.diagnostics.push(PluginDiagnostic::warning_for_path(
                    ctx.file.to_path_buf(),
                    format!(
                        "all arguments to verifies() in test '{test_name}' are non-string literals; dropping record",
                    ),
                ));
            }
            continue;
        }

        let targets = VerificationTargets::new(extraction.refs).expect("refs is non-empty");
        let (line, column) = offset_to_line_col(ctx.source, span_start);

        let annotation_span = SourceLocation {
            file: ctx.file.to_path_buf(),
            line,
            column,
        };

        let record = VerificationEvidenceRecord {
            id: EvidenceId::new(*ctx.next_id),
            targets,
            test: TestIdentity {
                file: ctx.file.to_path_buf(),
                name: test_name,
                kind: TestKind::Unit,
            },
            source_location: annotation_span.clone(),
            provenance: vec![PluginProvenance::JsVerifies { annotation_span }],
            metadata: BTreeMap::new(),
        };
        *ctx.next_id += 1;
        ctx.result.evidence.push(record);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// AST helpers
// ---------------------------------------------------------------------------

/// Find a `verifies()` `CallExpression` in the arguments of a test/it call.
///
/// Recognizes two forms:
/// - Direct: `test('name', verifies(...), fn)` — the argument IS a `verifies()` call
/// - Spread: `test('name', { ...verifies(...) }, fn)` — the argument is an object
///   with a spread element containing a `verifies()` call
fn find_verifies_call<'a, 'b>(args: &'a [Argument<'b>]) -> Option<&'a CallExpression<'b>> {
    for arg in args {
        match arg {
            // Direct verifies() call as argument.
            Argument::CallExpression(call) => {
                if is_verifies_callee(&call.callee) {
                    return Some(call);
                }
            }
            // Object expression — look for spread containing verifies().
            Argument::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    if let ObjectPropertyKind::SpreadProperty(spread) = prop
                        && let Expression::CallExpression(call) = &spread.argument
                        && is_verifies_callee(&call.callee)
                    {
                        return Some(call);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Check whether an expression is an `Identifier` named `"verifies"`.
fn is_verifies_callee(expr: &Expression<'_>) -> bool {
    matches!(expr, Expression::Identifier(id) if &*id.name == "verifies")
}

/// Find a raw `{ meta: { verifies: [...] } }` object literal in the arguments.
///
/// Returns the `ArrayExpression` for the `verifies` array and the byte offset
/// of the containing object expression (for source location).
fn find_raw_meta_verifies<'a, 'b>(
    args: &'a [Argument<'b>],
) -> Option<(&'a ArrayExpression<'b>, u32)> {
    for arg in args {
        let Argument::ObjectExpression(obj) = arg else {
            continue;
        };
        // Look for a property named `meta`.
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                continue;
            };
            if !property_key_is(&p.key, "meta") {
                continue;
            }
            // `meta` value must be an ObjectExpression.
            let Expression::ObjectExpression(meta_obj) = &p.value else {
                continue;
            };
            // Look for a property named `verifies` inside meta.
            for meta_prop in &meta_obj.properties {
                let ObjectPropertyKind::ObjectProperty(mp) = meta_prop else {
                    continue;
                };
                if !property_key_is(&mp.key, "verifies") {
                    continue;
                }
                // `verifies` value must be an ArrayExpression.
                if let Expression::ArrayExpression(arr) = &mp.value {
                    return Some((arr, obj.span.start));
                }
            }
        }
    }
    None
}

/// Check whether a `PropertyKey` is a static identifier with the given name.
fn property_key_is(key: &PropertyKey<'_>, name: &str) -> bool {
    matches!(key, PropertyKey::StaticIdentifier(id) if &*id.name == name)
}

/// Result of extracting refs from a single `verifies()` call.
struct VerifiesExtraction {
    refs: BTreeSet<VerifiableRef>,
    diagnostics: Vec<PluginDiagnostic>,
    had_non_string_args: bool,
}

/// Extract `VerifiableRef`s from string literals. Shared logic for both
/// `verifies()` call arguments and raw `meta.verifies` array elements.
///
/// Each `(Option<&str>, context)` pair represents one element: `Some(value)` for
/// string literals, `None` for non-string elements. Malformed refs (missing `#`)
/// produce a fatal `PluginError::Discovery`.
fn extract_refs_from_strings<'a>(
    items: impl Iterator<Item = Option<&'a str>>,
    file: &Path,
    context: &str,
) -> Result<VerifiesExtraction, PluginError> {
    let mut refs = BTreeSet::new();
    let mut diagnostics = Vec::new();
    let mut had_non_string_args = false;

    for item in items {
        if let Some(raw) = item {
            match VerifiableRef::parse(raw) {
                Some(vref) => {
                    refs.insert(vref);
                }
                None => {
                    return Err(PluginError::Discovery {
                        plugin: PLUGIN_NAME.to_string(),
                        message: format!(
                            "malformed criterion ref '{raw}' in {context} (missing '#')",
                        ),
                        details: Some(Box::new(PluginErrorDetails {
                            path: Some(file.to_path_buf()),
                            ..PluginErrorDetails::default()
                        })),
                    });
                }
            }
        } else {
            had_non_string_args = true;
            diagnostics.push(PluginDiagnostic::warning_for_path(
                file.to_path_buf(),
                format!("non-string element in {context}; only string literals are supported"),
            ));
        }
    }

    Ok(VerifiesExtraction {
        refs,
        diagnostics,
        had_non_string_args,
    })
}

/// Extract `VerifiableRef`s from the arguments of a `verifies()` call.
fn extract_verifies_refs(
    call: &CallExpression<'_>,
    file: &Path,
) -> Result<VerifiesExtraction, PluginError> {
    let items = call.arguments.iter().map(|arg| {
        if let Argument::StringLiteral(s) = arg {
            Some(s.value.as_str())
        } else {
            None
        }
    });
    extract_refs_from_strings(items, file, "verifies() call")
}

/// Extract `VerifiableRef`s from an `ArrayExpression` (the raw `verifies: [...]` form).
fn extract_array_refs(
    array: &ArrayExpression<'_>,
    file: &Path,
) -> Result<VerifiesExtraction, PluginError> {
    use oxc_ast::ast::ArrayExpressionElement;

    let items = array.elements.iter().map(|element| {
        if let ArrayExpressionElement::StringLiteral(s) = element {
            Some(s.value.as_str())
        } else {
            None
        }
    });
    extract_refs_from_strings(items, file, "meta.verifies array")
}

fn offset_to_line_col(source: &str, offset: u32) -> (usize, usize) {
    supersigil_parser::util::line_col(source, offset as usize)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use supersigil_rust_macros::verifies;

    // -----------------------------------------------------------------------
    // name()
    // -----------------------------------------------------------------------

    #[test]
    fn plugin_name_returns_js() {
        let plugin = JsPlugin;
        assert_eq!(plugin.name(), "js");
    }

    // -----------------------------------------------------------------------
    // plan_discovery_inputs integration
    // -----------------------------------------------------------------------

    #[test]
    #[verifies("js-plugin/req#req-1-6", "js-plugin/req#req-3-1")]
    fn plan_discovery_inputs_filters_by_extension() {
        let plugin = JsPlugin;
        let ts = PathBuf::from("/project/tests/auth.test.ts");
        let tsx = PathBuf::from("/project/tests/component.test.tsx");
        let js = PathBuf::from("/project/tests/legacy.test.js");
        let jsx = PathBuf::from("/project/tests/utils.spec.jsx");
        let rs = PathBuf::from("/project/tests/rust_test.rs");
        let json = PathBuf::from("/project/tests/data.json");
        let shared = vec![ts.clone(), tsx.clone(), js.clone(), jsx.clone(), rs, json];
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from("/project"),
        };

        let result = plugin.plan_discovery_inputs(&shared, &scope);

        assert_eq!(result.as_ref(), &[ts, tsx, js, jsx]);
    }

    #[test]
    #[verifies("js-plugin/req#req-3-2")]
    fn plan_discovery_inputs_empty_when_no_js_files() {
        let plugin = JsPlugin;
        let shared: Vec<PathBuf> = vec![
            PathBuf::from("/project/tests/foo.rs"),
            PathBuf::from("/project/tests/bar.py"),
        ];
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from("/project"),
        };

        let result = plugin.plan_discovery_inputs(&shared, &scope);
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // discover – fault tolerance
    // -----------------------------------------------------------------------

    fn empty_graph() -> DocumentGraph {
        let config = supersigil_core::Config {
            paths: Some(vec![]),
            ..supersigil_core::Config::default()
        };
        supersigil_core::build_graph(vec![], &config).unwrap()
    }

    /// Path to the test fixtures directory.
    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn discover_empty_file_list_returns_empty_result() {
        let plugin = JsPlugin;
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from("/tmp"),
        };
        let graph = empty_graph();
        let result = plugin.discover(&[], &scope, &graph).unwrap();
        assert!(result.evidence.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    #[verifies("js-plugin/req#req-4-1")]
    fn discover_syntax_error_produces_diagnostic_and_skips_file() {
        let plugin = JsPlugin;
        let scope = ProjectScope {
            project: None,
            project_root: fixtures_dir(),
        };
        let graph = empty_graph();
        let files = vec![fixtures_dir().join("syntax_error.test.ts")];

        let result = plugin.discover(&files, &scope, &graph).unwrap();

        // No evidence extracted from a file with syntax errors.
        assert!(result.evidence.is_empty());
        // A diagnostic should be emitted for the parse failure.
        assert_eq!(result.diagnostics.len(), 1);
        let diag = &result.diagnostics[0];
        assert!(
            diag.path
                .as_ref()
                .is_some_and(|p| p.ends_with("syntax_error.test.ts")),
            "diagnostic should reference the failing file, got: {:?}",
            diag.path
        );
        assert!(
            diag.message.contains("parse"),
            "diagnostic message should mention parsing, got: {:?}",
            diag.message
        );
    }

    #[test]
    #[verifies("js-plugin/req#req-4-2")]
    fn discover_clean_file_returns_empty_evidence() {
        let plugin = JsPlugin;
        let scope = ProjectScope {
            project: None,
            project_root: fixtures_dir(),
        };
        let graph = empty_graph();
        let files = vec![fixtures_dir().join("clean.test.ts")];

        let result = plugin.discover(&files, &scope, &graph).unwrap();

        assert!(result.evidence.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn discover_empty_file_returns_empty_evidence() {
        let plugin = JsPlugin;
        let scope = ProjectScope {
            project: None,
            project_root: fixtures_dir(),
        };
        let graph = empty_graph();
        let files = vec![fixtures_dir().join("empty.test.ts")];

        let result = plugin.discover(&files, &scope, &graph).unwrap();

        assert!(result.evidence.is_empty());
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn discover_mixed_files_skips_broken_continues_clean() {
        let plugin = JsPlugin;
        let scope = ProjectScope {
            project: None,
            project_root: fixtures_dir(),
        };
        let graph = empty_graph();
        let files = vec![
            fixtures_dir().join("syntax_error.test.ts"),
            fixtures_dir().join("clean.test.ts"),
        ];

        let result = plugin.discover(&files, &scope, &graph).unwrap();

        // Should still succeed overall.
        assert!(result.evidence.is_empty());
        // Exactly one diagnostic for the broken file.
        assert_eq!(result.diagnostics.len(), 1);
        assert!(
            result.diagnostics[0]
                .path
                .as_ref()
                .is_some_and(|p| p.ends_with("syntax_error.test.ts")),
        );
    }

    #[test]
    fn discover_nonexistent_file_produces_diagnostic() {
        let plugin = JsPlugin;
        let scope = ProjectScope {
            project: None,
            project_root: fixtures_dir(),
        };
        let graph = empty_graph();
        let files = vec![fixtures_dir().join("does_not_exist.test.ts")];

        let result = plugin.discover(&files, &scope, &graph).unwrap();

        assert!(result.evidence.is_empty());
        assert_eq!(result.diagnostics.len(), 1);
        assert!(
            result.diagnostics[0]
                .path
                .as_ref()
                .is_some_and(|p| p.ends_with("does_not_exist.test.ts")),
        );
    }

    // -----------------------------------------------------------------------
    // discover – verifies() extraction
    // -----------------------------------------------------------------------

    /// Helper: run discover on a single fixture file.
    fn discover_fixture(name: &str) -> Result<PluginDiscoveryResult, PluginError> {
        let plugin = JsPlugin;
        let scope = ProjectScope {
            project: None,
            project_root: fixtures_dir(),
        };
        let graph = empty_graph();
        let files = vec![fixtures_dir().join(name)];
        plugin.discover(&files, &scope, &graph)
    }

    #[test]
    #[verifies(
        "js-plugin/req#req-1-1",
        "js-plugin/req#req-1-2",
        "js-plugin/req#req-2-1"
    )]
    fn discover_single_ref() {
        let result = discover_fixture("single_ref.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1, "expected 1 evidence record");
        assert!(result.diagnostics.is_empty(), "expected no diagnostics");

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "creates user");
        assert_eq!(record.test.kind, TestKind::Unit);
        assert!(record.test.file.ends_with("single_ref.test.ts"));

        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);

        // Source location should point to the verifies() call on line 4.
        assert_eq!(record.source_location.line, 4);
    }

    #[test]
    #[verifies("js-plugin/req#req-2-2")]
    fn discover_multiple_refs() {
        let result = discover_fixture("multiple_refs.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1);
        assert!(result.diagnostics.is_empty());

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "handles auth");

        let mut targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        targets.sort();
        assert_eq!(targets, vec!["auth/req#req-1", "auth/req#req-2"]);
    }

    #[test]
    #[verifies("js-plugin/req#req-1-4")]
    fn discover_spread_form() {
        let result = discover_fixture("spread_form.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1);
        assert!(result.diagnostics.is_empty());

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "with timeout");

        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);
    }

    #[test]
    #[verifies("js-plugin/req#req-2-3")]
    fn discover_non_string_args() {
        let result = discover_fixture("non_string_args.test.ts").unwrap();

        // No evidence because there are no string literal refs.
        assert!(result.evidence.is_empty());

        // Should have diagnostics: one for the non-string arg, one for all-non-string drop.
        assert!(
            !result.diagnostics.is_empty(),
            "expected diagnostics for non-string args, got: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("non-string")),
            "expected non-string diagnostic, got: {:?}",
            result.diagnostics
        );
    }

    #[test]
    #[verifies("js-plugin/req#req-2-5")]
    fn discover_all_non_string_drops_record() {
        let result = discover_fixture("all_non_string.test.ts").unwrap();

        // No evidence record when all args are non-string.
        assert!(result.evidence.is_empty());

        // Should have diagnostics for each non-string arg plus the drop message.
        assert!(
            result.diagnostics.len() >= 2,
            "expected at least 2 diagnostics (non-string args), got: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("dropping record")),
            "expected 'dropping record' diagnostic, got: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn discover_mixed_string_and_non_string_args() {
        let result = discover_fixture("mixed_args.test.ts").unwrap();

        // The string ref should be kept, the non-string arg should produce a diagnostic.
        assert_eq!(
            result.evidence.len(),
            1,
            "expected 1 evidence record (string ref kept)"
        );

        let record = &result.evidence[0];
        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);

        // Should have a diagnostic for the non-string arg.
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("non-string")),
            "expected 'non-string' diagnostic, got: {:?}",
            result.diagnostics
        );
    }

    #[test]
    #[verifies("js-plugin/req#req-2-4")]
    fn discover_malformed_ref_returns_error() {
        let result = discover_fixture("malformed_ref.test.ts");

        assert!(result.is_err(), "expected PluginError::Discovery");
        let err = result.unwrap_err();
        match &err {
            PluginError::Discovery { message, .. } => {
                assert!(
                    message.contains("malformed") && message.contains("no-hash-here"),
                    "expected malformed ref error message, got: {message}",
                );
            }
            other => panic!("expected PluginError::Discovery, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // discover – raw meta.verifies extraction
    // -----------------------------------------------------------------------

    #[test]
    #[verifies("js-plugin/req#req-1-3")]
    fn discover_raw_meta_single_ref() {
        let result = discover_fixture("raw_meta_single.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1, "expected 1 evidence record");
        assert!(result.diagnostics.is_empty(), "expected no diagnostics");

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "creates user");
        assert_eq!(record.test.kind, TestKind::Unit);
        assert!(record.test.file.ends_with("raw_meta_single.test.ts"));

        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);

        // Source location should point to the object on line 3.
        assert_eq!(record.source_location.line, 3);
    }

    #[test]
    fn discover_raw_meta_multiple_refs() {
        let result = discover_fixture("raw_meta_multiple.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1, "expected 1 evidence record");
        assert!(result.diagnostics.is_empty(), "expected no diagnostics");

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "handles auth");

        let mut targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        targets.sort();
        assert_eq!(targets, vec!["auth/req#req-1", "auth/req#req-2"]);
    }

    #[test]
    fn discover_raw_meta_with_options() {
        let result = discover_fixture("raw_meta_with_options.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1, "expected 1 evidence record");
        assert!(result.diagnostics.is_empty(), "expected no diagnostics");

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "with timeout");

        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);
    }

    #[test]
    fn discover_it_alias() {
        let result = discover_fixture("it_alias.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1);
        assert!(result.diagnostics.is_empty());

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "uses it alias");

        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);
    }

    // -----------------------------------------------------------------------
    // discover – describe nesting
    // -----------------------------------------------------------------------

    #[test]
    #[verifies("js-plugin/req#req-1-5")]
    fn discover_nested_describe() {
        let result = discover_fixture("nested_describe.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1, "expected 1 evidence record");
        assert!(result.diagnostics.is_empty(), "expected no diagnostics");

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "auth > creates user");
        assert_eq!(record.test.kind, TestKind::Unit);
        assert!(record.test.file.ends_with("nested_describe.test.ts"));

        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);
    }

    #[test]
    fn discover_double_nested_describe() {
        let result = discover_fixture("double_nested_describe.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 1, "expected 1 evidence record");
        assert!(result.diagnostics.is_empty(), "expected no diagnostics");

        let record = &result.evidence[0];
        assert_eq!(record.test.name, "auth > login > succeeds");
        assert_eq!(record.test.kind, TestKind::Unit);
        assert!(record.test.file.ends_with("double_nested_describe.test.ts"));

        let targets: Vec<String> = record.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);
    }

    #[test]
    fn discover_mixed_top_and_nested() {
        let result = discover_fixture("mixed_top_and_nested.test.ts").unwrap();

        assert_eq!(result.evidence.len(), 2, "expected 2 evidence records");
        assert!(result.diagnostics.is_empty(), "expected no diagnostics");

        // First record: top-level test (no describe prefix).
        let top = &result.evidence[0];
        assert_eq!(top.test.name, "top level");
        let targets: Vec<String> = top.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-1"]);

        // Second record: nested test with describe prefix.
        let nested = &result.evidence[1];
        assert_eq!(nested.test.name, "suite > nested");
        let targets: Vec<String> = nested.targets.iter().map(ToString::to_string).collect();
        assert_eq!(targets, vec!["auth/req#req-2"]);
    }
}
