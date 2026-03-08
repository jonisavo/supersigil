//! `syn`-based source discovery for `#[verifies(...)]` attributes.
//!
//! Walks Rust source files, parses them with `syn`, and extracts
//! `verifies` attribute invocations to produce raw evidence records.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::TokenTree;
use syn::spanned::Spanned;

use supersigil_core::DocumentGraph;
use supersigil_evidence::{
    EcosystemPlugin, EvidenceId, EvidenceKind, PluginError, PluginProvenance, ProjectScope,
    SourceLocation, TestIdentity, TestKind, VerifiableRef, VerificationEvidenceRecord,
};

const PLUGIN_NAME: &str = "rust";

struct VerifiesParseError {
    message: String,
    span: proc_macro2::Span,
}

// ---------------------------------------------------------------------------
// RustPlugin — EcosystemPlugin implementation
// ---------------------------------------------------------------------------

/// Built-in Rust ecosystem plugin.
///
/// Discovers `#[verifies(...)]` evidence by parsing Rust source files
/// with `syn`.
#[derive(Debug)]
pub struct RustPlugin;

impl EcosystemPlugin for RustPlugin {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn discover(
        &self,
        files: &[PathBuf],
        _scope: &ProjectScope,
        _documents: &DocumentGraph,
    ) -> Result<Vec<VerificationEvidenceRecord>, PluginError> {
        let rust_files: Vec<&PathBuf> = files
            .iter()
            .filter(|file| file.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        if rust_files.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_records = Vec::new();
        let mut supported_test_items = 0usize;
        for file in rust_files {
            match discover_file_summary(file) {
                Ok(summary) => {
                    supported_test_items += summary.supported_test_items;
                    all_records.extend(summary.records);
                }
                Err(err) => {
                    eprintln!(
                        "warning: plugin '{PLUGIN_NAME}': skipping {}: {err}",
                        file.display()
                    );
                }
            }
        }

        if supported_test_items == 0 {
            return Err(PluginError::Discovery {
                plugin: PLUGIN_NAME.to_string(),
                message: "zero supported Rust test items were found in the discovery scope; supported forms include #[test], #[tokio::test], supported proptest wrappers, and snapshot-oriented tests".to_string(),
            });
        }

        Ok(all_records)
    }
}

// ---------------------------------------------------------------------------
// Discovery accumulator
// ---------------------------------------------------------------------------

/// Accumulates evidence records and counters during recursive item walking.
struct DiscoveryAccumulator {
    records: Vec<VerificationEvidenceRecord>,
    next_id: usize,
    supported_test_items: usize,
}

impl DiscoveryAccumulator {
    fn new() -> Self {
        Self {
            records: Vec::new(),
            next_id: 0,
            supported_test_items: 0,
        }
    }

    fn alloc_id(&mut self) -> EvidenceId {
        let id = EvidenceId(self.next_id);
        self.next_id += 1;
        id
    }
}

// ---------------------------------------------------------------------------
// File-level discovery
// ---------------------------------------------------------------------------

/// Discover `#[verifies(...)]` evidence in a single Rust source file.
///
/// Parses the file with `syn`, walks all item-level functions, and extracts
/// evidence records for each function annotated with `#[verifies(...)]`.
///
/// # Errors
///
/// Returns `PluginError` if the file cannot be read or parsed.
#[cfg(test)]
fn discover_file(path: &Path) -> Result<Vec<VerificationEvidenceRecord>, PluginError> {
    Ok(discover_file_summary(path)?.records)
}

struct FileDiscoverySummary {
    records: Vec<VerificationEvidenceRecord>,
    supported_test_items: usize,
}

fn discover_file_summary(path: &Path) -> Result<FileDiscoverySummary, PluginError> {
    let source = std::fs::read_to_string(path).map_err(|e| PluginError::Io {
        plugin: PLUGIN_NAME.to_string(),
        path: path.to_path_buf(),
        source: e,
    })?;

    // Cheap pre-filter: skip the expensive `syn::parse_file` when the source
    // cannot contain `#[verifies(...)]` attributes.  For the
    // `supported_test_items` diagnostic counter we fall back to a simple
    // string-contains heuristic instead of a full parse.
    let has_verifies = source.contains("verifies");
    if !has_verifies {
        let cheap_test_count = cheap_supported_test_count(&source);
        return Ok(FileDiscoverySummary {
            records: Vec::new(),
            supported_test_items: cheap_test_count,
        });
    }

    let syntax = syn::parse_file(&source).map_err(|e| PluginError::ParseFailure {
        plugin: PLUGIN_NAME.to_string(),
        file: path.to_path_buf(),
        message: e.to_string(),
    })?;

    let mut acc = DiscoveryAccumulator::new();
    collect_items(&syntax.items, path, &mut acc)?;

    Ok(FileDiscoverySummary {
        records: acc.records,
        supported_test_items: acc.supported_test_items,
    })
}

/// Cheap heuristic count of supported test items via string matching.
///
/// This avoids a full `syn` parse for files that cannot contain `#[verifies]`
/// attributes.  It is intentionally over-counting (e.g. matches inside
/// comments or strings) since the counter is only used for the "zero
/// supported test items" diagnostic — false positives are harmless.
fn cheap_supported_test_count(source: &str) -> usize {
    let mut count = 0usize;
    // Count occurrences of `#[test]` and `#[tokio::test]` — these cover
    // unit, async, and snapshot test items.
    count += source.matches("#[test]").count();
    count += source.matches("#[tokio::test]").count();
    // Count `proptest!` macro invocations.
    count += source.matches("proptest!").count();
    count
}

/// Walk a list of items, recursing into inline `mod` blocks.
fn collect_items(
    items: &[syn::Item],
    path: &Path,
    acc: &mut DiscoveryAccumulator,
) -> Result<(), PluginError> {
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => {
                if is_supported_fn_test(item_fn) {
                    acc.supported_test_items += 1;
                }
                if let Some(record) = process_fn(path, item_fn, acc)? {
                    acc.records.push(record);
                }
            }
            syn::Item::Macro(item_macro) => {
                if is_supported_macro_test(item_macro) {
                    acc.supported_test_items += 1;
                }
                if let Some(record) = process_macro(path, item_macro, acc)? {
                    acc.records.push(record);
                }
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, ref inner_items)) = item_mod.content {
                    collect_items(inner_items, path, acc)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared record builder
// ---------------------------------------------------------------------------

/// Build a `VerificationEvidenceRecord` from already-extracted fields.
fn build_record(
    acc: &mut DiscoveryAccumulator,
    path: &Path,
    targets: BTreeSet<VerifiableRef>,
    attr_span: proc_macro2::Span,
    test_name: String,
    test_kind: TestKind,
    metadata: BTreeMap<String, String>,
) -> VerificationEvidenceRecord {
    let start = attr_span.start();
    let source_location = SourceLocation {
        file: path.to_path_buf(),
        line: start.line,
        column: start.column + 1, // syn is 0-indexed, we want 1-indexed
    };

    VerificationEvidenceRecord {
        id: acc.alloc_id(),
        targets,
        test: TestIdentity {
            file: path.to_path_buf(),
            name: test_name,
            kind: test_kind,
        },
        source_location: source_location.clone(),
        evidence_kind: EvidenceKind::RustAttribute,
        provenance: vec![PluginProvenance::RustAttribute {
            attribute_span: source_location,
        }],
        metadata,
    }
}

/// Map a `VerifiesParseError` into a `PluginError::Discovery` with location info.
fn parse_error_to_plugin_error(path: &Path, error: &VerifiesParseError) -> PluginError {
    PluginError::Discovery {
        plugin: PLUGIN_NAME.to_string(),
        message: format!(
            "{}:{}:{}: {}",
            path.display(),
            error.span.start().line,
            error.span.start().column + 1,
            error.message
        ),
    }
}

// ---------------------------------------------------------------------------
// Item processors
// ---------------------------------------------------------------------------

/// Process a function item, looking for `#[verifies(...)]` attributes.
fn process_fn(
    path: &Path,
    item_fn: &syn::ItemFn,
    acc: &mut DiscoveryAccumulator,
) -> Result<Option<VerificationEvidenceRecord>, PluginError> {
    let Some((targets, attr_span)) = extract_verifies_targets(&item_fn.attrs)
        .map_err(|e| parse_error_to_plugin_error(path, &e))?
    else {
        return Ok(None);
    };

    let Some(test_kind) = determine_fn_test_kind(item_fn) else {
        return Ok(None);
    };
    let metadata = extract_fn_metadata(item_fn, test_kind);

    Ok(Some(build_record(
        acc,
        path,
        targets,
        attr_span,
        item_fn.sig.ident.to_string(),
        test_kind,
        metadata,
    )))
}

/// Process a macro invocation item (e.g., `proptest! { ... }`), looking for
/// `#[verifies(...)]` outer attributes on the macro item.
fn process_macro(
    path: &Path,
    item_macro: &syn::ItemMacro,
    acc: &mut DiscoveryAccumulator,
) -> Result<Option<VerificationEvidenceRecord>, PluginError> {
    let Some((targets, attr_span)) = extract_verifies_targets(&item_macro.attrs)
        .map_err(|e| parse_error_to_plugin_error(path, &e))?
    else {
        return Ok(None);
    };

    let macro_name = item_macro
        .mac
        .path
        .segments
        .last()
        .map(|seg| seg.ident.to_string())
        .unwrap_or_default();

    let Some(fn_name) = extract_fn_name_from_macro_tokens(&item_macro.mac.tokens) else {
        return Ok(None);
    };

    let (test_kind, metadata) = if macro_name == "proptest" {
        let mut meta = BTreeMap::new();
        meta.insert("framework".to_string(), "proptest".to_string());
        (TestKind::Property, meta)
    } else {
        return Ok(None);
    };

    Ok(Some(build_record(
        acc, path, targets, attr_span, fn_name, test_kind, metadata,
    )))
}

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------

/// Extract `VerifiableRef` targets from `#[verifies(...)]` attributes.
///
/// Returns `None` if no matching attribute is found.
/// Returns the set of criterion refs and the span of the attribute.
fn extract_verifies_targets(
    attrs: &[syn::Attribute],
) -> Result<Option<(BTreeSet<VerifiableRef>, proc_macro2::Span)>, VerifiesParseError> {
    for attr in attrs {
        let path = attr.path();
        let is_verifies = path.is_ident("verifies")
            || (path.segments.len() == 2
                && path.segments[0].ident == "supersigil_rust"
                && path.segments[1].ident == "verifies");
        if is_verifies {
            let span = attr.span();
            let mut targets = BTreeSet::new();
            let mut saw_valid_ref = false;

            // Parse the attribute arguments: verifies("ref1", "ref2", ...)
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = &meta_list.tokens;
                for token in tokens.clone() {
                    if let TokenTree::Literal(lit) = token {
                        let raw = lit.to_string();
                        if !raw.starts_with('"') || !raw.ends_with('"') {
                            return Err(VerifiesParseError {
                                message: "`#[verifies(...)]` expects string literal refs"
                                    .to_string(),
                                span,
                            });
                        }

                        let value = raw.trim_matches('"');
                        if let Some(verifiable_ref) = VerifiableRef::parse(value) {
                            targets.insert(verifiable_ref);
                            saw_valid_ref = true;
                        } else if !value.is_empty() {
                            // Fragmentless document ref — accepted but produces
                            // no criterion targets (document-level evidence).
                            saw_valid_ref = true;
                        } else {
                            return Err(VerifiesParseError {
                                message: "empty verifies reference".to_string(),
                                span,
                            });
                        }
                    }
                }
            }

            if saw_valid_ref {
                return Ok(Some((targets, span)));
            }

            return Err(VerifiesParseError {
                message: "`#[verifies(...)]` requires at least one string literal ref".to_string(),
                span,
            });
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Test kind classification
// ---------------------------------------------------------------------------

fn is_supported_fn_test(item_fn: &syn::ItemFn) -> bool {
    determine_fn_test_kind(item_fn).is_some()
}

fn is_supported_macro_test(item_macro: &syn::ItemMacro) -> bool {
    item_macro
        .mac
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "proptest")
}

fn determine_fn_test_kind(item_fn: &syn::ItemFn) -> Option<TestKind> {
    // Check for async test frameworks first (e.g., #[tokio::test]).
    if has_async_test_attr(&item_fn.attrs) {
        return Some(TestKind::Async);
    }

    // A bare `#[test]` attribute is required before classifying further.
    // Without it, the function is a helper, not a test.
    if !has_test_attr(&item_fn.attrs) {
        return None;
    }

    // Classify by kind: async test, snapshot test, or plain unit test.
    if item_fn.sig.asyncness.is_some() {
        return Some(TestKind::Async);
    }
    if body_contains_insta_snapshot(&item_fn.block) {
        return Some(TestKind::Snapshot);
    }
    Some(TestKind::Unit)
}

fn has_async_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        segments.len() == 2 && segments[1] == "test" && segments[0] == "tokio"
    })
}

fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("test"))
}

fn body_contains_insta_snapshot(block: &syn::Block) -> bool {
    for stmt in &block.stmts {
        if let syn::Stmt::Macro(stmt_macro) = stmt {
            let path_str = path_to_string(&stmt_macro.mac.path);
            if path_str == "insta::assert_snapshot" {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Metadata extraction
// ---------------------------------------------------------------------------

fn extract_fn_metadata(item_fn: &syn::ItemFn, test_kind: TestKind) -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();

    if test_kind == TestKind::Snapshot {
        metadata.insert("framework".to_string(), "insta".to_string());
        if let Some(snapshot_name) = extract_insta_snapshot_name(&item_fn.block) {
            metadata.insert("snapshot_name".to_string(), snapshot_name);
        }
    }

    metadata
}

fn extract_insta_snapshot_name(block: &syn::Block) -> Option<String> {
    for stmt in &block.stmts {
        if let syn::Stmt::Macro(stmt_macro) = stmt {
            let mac = &stmt_macro.mac;
            let path_str = path_to_string(&mac.path);
            if path_str == "insta::assert_snapshot" {
                for token in mac.tokens.clone() {
                    if let TokenTree::Literal(lit) = token {
                        let raw = lit.to_string();
                        if raw.starts_with('"') {
                            return Some(raw.trim_matches('"').to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

fn extract_fn_name_from_macro_tokens(tokens: &proc_macro2::TokenStream) -> Option<String> {
    let mut iter = tokens.clone().into_iter();
    while let Some(token) = iter.next() {
        if let TokenTree::Ident(ident) = &token
            && ident == "fn"
            && let Some(TokenTree::Ident(name)) = iter.next()
        {
            return Some(name.to_string());
        }
        if let TokenTree::Group(group) = token
            && let Some(name) = extract_fn_name_from_macro_tokens(&group.stream())
        {
            return Some(name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Return the path to a fixture file under `tests/fixtures/discover/`.
    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/discover")
            .join(name)
    }

    // -----------------------------------------------------------------------
    // Unit test discovery (req-6-1, req-6-2, req-6-3)
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_unit_test_with_verifies() {
        let records = discover_file(&fixture("unit_test.rs")).unwrap();

        assert_eq!(records.len(), 1, "expected exactly one evidence record");
        let record = &records[0];

        // Test identity (req-6-3)
        assert_eq!(record.test.name, "test_login_succeeds");
        assert_eq!(record.test.kind, TestKind::Unit);
        assert_eq!(record.test.file, fixture("unit_test.rs"));

        // Criterion targets (req-6-3)
        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        }]);
        assert_eq!(record.targets, expected_targets);

        // Evidence kind (req-6-1)
        assert_eq!(record.evidence_kind, EvidenceKind::RustAttribute);

        // Source location (req-6-3): the `#[verifies(...)]` attribute is on line 3
        assert_eq!(record.source_location.file, fixture("unit_test.rs"));
        assert_eq!(record.source_location.line, 3);
        assert!(
            record.source_location.column > 0,
            "expected column to be > 0, got {}",
            record.source_location.column
        );
    }

    // -----------------------------------------------------------------------
    // Async test discovery (req-6-2, req-6-3)
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_async_test_with_tokio() {
        let records = discover_file(&fixture("async_test.rs")).unwrap();

        assert_eq!(records.len(), 1, "expected exactly one evidence record");
        let record = &records[0];

        assert_eq!(record.test.name, "test_api_call");
        assert_eq!(record.test.kind, TestKind::Async);
        assert_eq!(record.test.file, fixture("async_test.rs"));

        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/api".to_string(),
            target_id: "crit-1".to_string(),
        }]);
        assert_eq!(record.targets, expected_targets);
        assert_eq!(record.evidence_kind, EvidenceKind::RustAttribute);
        assert_eq!(record.source_location.line, 3);
    }

    // -----------------------------------------------------------------------
    // Non-test functions must NOT produce evidence
    // -----------------------------------------------------------------------

    #[test]
    fn async_helper_with_verifies_produces_no_evidence() {
        let records = discover_file(&fixture("async_helper.rs")).unwrap();
        assert!(
            records.is_empty(),
            "async helper without #[test] should produce no evidence, got {} records",
            records.len(),
        );
    }

    // -----------------------------------------------------------------------
    // Proptest discovery (req-6-2, req-6-3, req-6-4)
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_proptest_with_verifies() {
        let records = discover_file(&fixture("proptest_test.rs")).unwrap();

        assert_eq!(records.len(), 1, "expected exactly one evidence record");
        let record = &records[0];

        assert_eq!(record.test.name, "test_roundtrip");
        assert_eq!(record.test.kind, TestKind::Property);
        assert_eq!(record.test.file, fixture("proptest_test.rs"));

        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/validation".to_string(),
            target_id: "crit-1".to_string(),
        }]);
        assert_eq!(record.targets, expected_targets);
        assert_eq!(record.evidence_kind, EvidenceKind::RustAttribute);
        assert_eq!(record.source_location.line, 3);

        assert_eq!(
            record.metadata.get("framework").map(String::as_str),
            Some("proptest"),
        );
    }

    // -----------------------------------------------------------------------
    // Snapshot test discovery (req-6-2, req-6-3, req-6-4)
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_snapshot_test_with_insta() {
        let records = discover_file(&fixture("snapshot_test.rs")).unwrap();

        assert_eq!(records.len(), 1, "expected exactly one evidence record");
        let record = &records[0];

        assert_eq!(record.test.name, "test_render_output");
        assert_eq!(record.test.kind, TestKind::Snapshot);
        assert_eq!(record.test.file, fixture("snapshot_test.rs"));

        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/output".to_string(),
            target_id: "crit-1".to_string(),
        }]);
        assert_eq!(record.targets, expected_targets);
        assert_eq!(record.evidence_kind, EvidenceKind::RustAttribute);
        assert_eq!(record.source_location.line, 3);

        assert_eq!(
            record.metadata.get("framework").map(String::as_str),
            Some("insta"),
        );
        assert_eq!(
            record.metadata.get("snapshot_name").map(String::as_str),
            Some("render_output"),
        );
    }

    // -----------------------------------------------------------------------
    // Multiple criterion refs (req-6-3)
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_multiple_targets() {
        let records = discover_file(&fixture("multiple_attrs.rs")).unwrap();

        assert_eq!(records.len(), 1, "expected exactly one evidence record");
        let record = &records[0];

        assert_eq!(record.test.name, "test_full_auth_flow");
        assert_eq!(record.test.kind, TestKind::Unit);

        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([
            VerifiableRef {
                doc_id: "req/auth".to_string(),
                target_id: "crit-1".to_string(),
            },
            VerifiableRef {
                doc_id: "req/auth".to_string(),
                target_id: "crit-2".to_string(),
            },
            VerifiableRef {
                doc_id: "req/security".to_string(),
                target_id: "crit-3".to_string(),
            },
        ]);
        assert_eq!(record.targets, expected_targets);
        assert_eq!(record.evidence_kind, EvidenceKind::RustAttribute);
    }

    // -----------------------------------------------------------------------
    // Document-level refs (Phase 6)
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_document_level_ref_with_empty_targets() {
        let records = discover_file(&fixture("doc_level_ref.rs")).unwrap();

        assert_eq!(records.len(), 1, "expected exactly one evidence record");
        let record = &records[0];

        assert_eq!(record.test.name, "test_auth_basics");
        assert_eq!(record.test.kind, TestKind::Unit);
        assert!(
            record.targets.is_empty(),
            "document-level ref should produce empty targets, got {:?}",
            record.targets,
        );
        assert_eq!(record.evidence_kind, EvidenceKind::RustAttribute);
    }

    // -----------------------------------------------------------------------
    // Path-qualified attribute: #[supersigil_rust::verifies(...)]
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_path_qualified_verifies_attribute() {
        let records = discover_file(&fixture("path_qualified_attr.rs")).unwrap();

        assert_eq!(records.len(), 1, "expected exactly one evidence record");
        let record = &records[0];

        assert_eq!(record.test.name, "test_path_qualified");
        assert_eq!(record.test.kind, TestKind::Unit);
        assert_eq!(record.test.file, fixture("path_qualified_attr.rs"));

        let expected_targets: BTreeSet<VerifiableRef> = BTreeSet::from([VerifiableRef {
            doc_id: "req/auth".to_string(),
            target_id: "crit-1".to_string(),
        }]);
        assert_eq!(record.targets, expected_targets);
        assert_eq!(record.evidence_kind, EvidenceKind::RustAttribute);
        assert_eq!(record.source_location.line, 1);
    }

    // -----------------------------------------------------------------------
    // No evidence for unannotated functions (req-6-1)
    // -----------------------------------------------------------------------

    #[test]
    fn no_evidence_for_tests_without_verifies() {
        let records = discover_file(&fixture("no_verifies.rs")).unwrap();

        assert!(
            records.is_empty(),
            "expected no evidence records for unannotated tests, got {}",
            records.len()
        );
    }

    // -----------------------------------------------------------------------
    // Provenance tracking (req-6-1, req-6-3)
    // -----------------------------------------------------------------------

    #[test]
    fn unit_test_provenance_is_rust_attribute() {
        let records = discover_file(&fixture("unit_test.rs")).unwrap();

        assert_eq!(records.len(), 1);
        let record = &records[0];

        assert!(
            !record.provenance.is_empty(),
            "expected at least one provenance entry"
        );
        assert!(
            record
                .provenance
                .iter()
                .any(|p| matches!(p, PluginProvenance::RustAttribute { .. })),
            "expected at least one RustAttribute provenance entry, got {:?}",
            record.provenance
        );
    }

    // -----------------------------------------------------------------------
    // Source location accuracy (req-6-3)
    // -----------------------------------------------------------------------

    #[test]
    fn async_test_source_location_has_correct_file() {
        let records = discover_file(&fixture("async_test.rs")).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_location.file, fixture("async_test.rs"));
    }

    #[test]
    fn snapshot_test_source_location_line() {
        let records = discover_file(&fixture("snapshot_test.rs")).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_location.line, 3);
    }

    #[test]
    fn multiple_attrs_source_location_line() {
        let records = discover_file(&fixture("multiple_attrs.rs")).unwrap();

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].source_location.line, 3);
    }

    // -----------------------------------------------------------------------
    // Mod block recursion (functions inside `mod tests { ... }`)
    // -----------------------------------------------------------------------

    #[test]
    fn discovers_tests_inside_mod_block() {
        let records = discover_file(&fixture("mod_block_test.rs")).unwrap();

        assert_eq!(
            records.len(),
            2,
            "expected 2 evidence records from mod block, got {}",
            records.len()
        );

        let names: Vec<&str> = records.iter().map(|r| r.test.name.as_str()).collect();
        assert!(
            names.contains(&"test_inside_mod"),
            "should discover test inside mod block, got: {names:?}",
        );
        assert!(
            names.contains(&"test_async_inside_mod"),
            "should discover async test inside mod block, got: {names:?}",
        );

        let unit = records
            .iter()
            .find(|r| r.test.name == "test_inside_mod")
            .unwrap();
        assert_eq!(unit.test.kind, TestKind::Unit);

        let async_test = records
            .iter()
            .find(|r| r.test.name == "test_async_inside_mod")
            .unwrap();
        assert_eq!(async_test.test.kind, TestKind::Async);

        assert!(unit.targets.iter().any(|c| c.target_id == "crit-1"));
        assert!(async_test.targets.iter().any(|c| c.target_id == "crit-2"));
    }

    // -----------------------------------------------------------------------
    // RustPlugin trait implementation (req-10-3)
    // -----------------------------------------------------------------------

    fn empty_graph() -> DocumentGraph {
        let config = supersigil_core::Config {
            paths: Some(vec![]),
            ..supersigil_core::Config::default()
        };
        supersigil_core::build_graph(vec![], &config).unwrap()
    }

    #[test]
    fn rust_plugin_discovers_across_multiple_files() {
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let graph = empty_graph();

        let files = vec![
            fixture("unit_test.rs"),
            fixture("async_test.rs"),
            fixture("no_verifies.rs"),
        ];

        let plugin = RustPlugin;
        assert_eq!(plugin.name(), PLUGIN_NAME);

        let records = plugin.discover(&files, &scope, &graph).unwrap();
        assert_eq!(records.len(), 2, "expected 2 evidence records from 3 files");
    }

    #[test]
    fn rust_plugin_continues_past_per_file_errors() {
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let graph = empty_graph();

        let files = vec![
            fixture("unit_test.rs"),
            // This file does not exist — should be skipped, not abort discovery
            fixture("this_file_does_not_exist.rs"),
            fixture("async_test.rs"),
        ];

        let plugin = RustPlugin;
        let records = plugin.discover(&files, &scope, &graph).unwrap();
        assert_eq!(
            records.len(),
            2,
            "should discover 2 records despite 1 missing file, got {}",
            records.len(),
        );
    }

    #[test]
    fn rust_plugin_skips_non_rs_files() {
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let graph = empty_graph();

        let files = vec![
            fixture("unit_test.rs"),
            PathBuf::from("some_file.txt"),
            PathBuf::from("Cargo.toml"),
        ];

        let plugin = RustPlugin;
        let records = plugin.discover(&files, &scope, &graph).unwrap();
        assert_eq!(records.len(), 1, "should only discover from .rs files");
    }

    /// Empty file list should return Ok(empty), not an error.
    /// Non-Rust repos or mixed workspaces may legitimately have no .rs files
    /// in the discovery scope.
    #[test]
    fn rust_plugin_returns_empty_for_no_rs_files() {
        let scope = ProjectScope {
            project: None,
            project_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        };
        let graph = empty_graph();
        let plugin = RustPlugin;

        // Pass only non-Rust files
        let files = vec![PathBuf::from("README.md"), PathBuf::from("Cargo.toml")];
        let result = plugin.discover(&files, &scope, &graph);

        assert!(
            result.is_ok(),
            "empty Rust scope should return Ok, not Err: {result:?}",
        );
        assert!(
            result.unwrap().is_empty(),
            "should produce zero evidence for non-Rust files",
        );
    }

    #[test]
    fn rust_plugin_errors_when_no_supported_test_items_are_found() {
        let dir = std::env::temp_dir().join("supersigil_test_no_supported_items");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("helpers.rs");
        std::fs::write(
            &path,
            "use supersigil_rust::verifies;\n#[verifies(\"req/auth#crit-1\")]\nfn helper() {}\n",
        )
        .unwrap();

        let scope = ProjectScope {
            project: None,
            project_root: dir.clone(),
        };
        let graph = empty_graph();
        let plugin = RustPlugin;

        let result = plugin.discover(std::slice::from_ref(&path), &scope, &graph);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);

        assert!(result.is_err(), "expected discovery error");
        let err = result.unwrap_err();
        assert!(
            matches!(err, PluginError::Discovery { .. }),
            "expected PluginError::Discovery, got {err:?}",
        );
        assert!(
            err.to_string().contains("zero supported Rust test items"),
            "unexpected error message: {err}",
        );
    }

    #[test]
    fn fragmentless_ref_accepted_as_document_level_evidence() {
        let dir = std::env::temp_dir().join("supersigil_test_doc_level_ref");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("auth_test.rs");
        std::fs::write(
            &path,
            "use supersigil_rust::verifies;\n#[test]\n#[verifies(\"req/auth\")]\nfn login_succeeds() {}\n",
        )
        .unwrap();

        let scope = ProjectScope {
            project: None,
            project_root: dir.clone(),
        };
        let graph = empty_graph();
        let plugin = RustPlugin;

        let result = plugin.discover(std::slice::from_ref(&path), &scope, &graph);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);

        let records = result.expect("document-level ref should be accepted");
        assert_eq!(records.len(), 1, "expected one evidence record");
        assert!(
            records[0].targets.is_empty(),
            "document-level ref should have empty targets, got {:?}",
            records[0].targets,
        );
        assert_eq!(records[0].test.name, "login_succeeds");
    }

    // -----------------------------------------------------------------------
    // Error-path tests
    // -----------------------------------------------------------------------

    #[test]
    fn discover_nonexistent_file_returns_io_error() {
        let path = fixture("this_file_does_not_exist.rs");
        let result = discover_file(&path);

        assert!(result.is_err(), "expected Err for nonexistent file");
        assert!(
            matches!(result.unwrap_err(), PluginError::Io { .. }),
            "expected PluginError::Io variant"
        );
    }

    #[test]
    fn discover_invalid_syntax_returns_parse_error() {
        let dir = std::env::temp_dir().join("supersigil_test_invalid_syntax");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_syntax.rs");
        std::fs::write(&path, "#[verifies(\"req/a#c\")] fn { broken").unwrap();

        let result = discover_file(&path);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);

        assert!(result.is_err(), "expected Err for invalid syntax");
        assert!(
            matches!(result.unwrap_err(), PluginError::ParseFailure { .. }),
            "expected PluginError::ParseFailure variant"
        );
    }
}
