//! `syn`-based source discovery for `#[verifies(...)]` attributes.
//!
//! Walks Rust source files, parses them with `syn`, and extracts
//! `verifies` attribute invocations to produce raw evidence records.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::TokenTree;
use syn::spanned::Spanned;

use supersigil_core::DocumentGraph;
use supersigil_evidence::{
    EcosystemPlugin, EvidenceId, PluginDiagnostic, PluginDiscoveryResult, PluginError,
    PluginErrorDetails, PluginProvenance, ProjectScope, SourceLocation, TestIdentity, TestKind,
    VerifiableRef, VerificationEvidenceRecord, VerificationTargets,
};

const PLUGIN_NAME: &str = "rust";

struct VerifiesParseError {
    message: String,
    span: proc_macro2::Span,
}

const RUST_DISCOVERY_DIRS: [&str; 4] = ["tests", "src", "benches", "examples"];

// ---------------------------------------------------------------------------
// Discovery input planning
// ---------------------------------------------------------------------------

fn plan_rust_discovery_inputs<'a>(
    test_files: &'a [PathBuf],
    project_root: &Path,
) -> Cow<'a, [PathBuf]> {
    let mut files: BTreeSet<PathBuf> = test_files.iter().cloned().collect();

    let roots =
        std::iter::once(project_root.to_path_buf()).chain(read_workspace_member_dirs(project_root));
    for root in roots {
        for dir in RUST_DISCOVERY_DIRS {
            glob_rs_files(&root.join(dir), &mut files);
        }
    }

    Cow::Owned(files.into_iter().collect())
}

fn glob_rs_files(dir: &Path, files: &mut BTreeSet<PathBuf>) {
    let pattern = dir.join("**/*.rs").to_string_lossy().to_string();
    if let Ok(entries) = glob::glob(&pattern) {
        for entry in entries.flatten() {
            if !path_contains_fixture_dir(&entry) {
                files.insert(entry);
            }
        }
    }
}

fn path_contains_fixture_dir(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "fixtures")
}

fn read_workspace_member_dirs(project_root: &Path) -> Vec<PathBuf> {
    let cargo_path = project_root.join("Cargo.toml");
    let Ok(content) = std::fs::read_to_string(&cargo_path) else {
        return Vec::new();
    };
    let Ok(table) = content.parse::<toml::Table>() else {
        return Vec::new();
    };

    let Some(members) = table
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(|members| members.as_array())
    else {
        return Vec::new();
    };

    let mut dirs = Vec::new();
    for member in members {
        let Some(pattern) = member.as_str() else {
            continue;
        };
        let full = project_root.join(pattern).to_string_lossy().to_string();
        if let Ok(entries) = glob::glob(&full) {
            for entry in entries.flatten() {
                if entry.is_dir() {
                    dirs.push(entry);
                }
            }
        } else {
            let dir = project_root.join(pattern);
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }

    dirs
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

    fn plan_discovery_inputs<'a>(
        &self,
        test_files: &'a [PathBuf],
        scope: &ProjectScope,
    ) -> Cow<'a, [PathBuf]> {
        plan_rust_discovery_inputs(test_files, &scope.project_root)
    }

    fn discover(
        &self,
        files: &[PathBuf],
        _scope: &ProjectScope,
        _documents: &DocumentGraph,
    ) -> Result<PluginDiscoveryResult, PluginError> {
        let rust_files: Vec<&PathBuf> = files
            .iter()
            .filter(|file| file.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        if rust_files.is_empty() {
            return Ok(PluginDiscoveryResult::default());
        }

        let mut result = PluginDiscoveryResult::default();
        let mut supported_test_items = 0usize;
        let mut first_error = None;
        for file in rust_files {
            match discover_file_summary(file) {
                Ok(summary) => {
                    supported_test_items += summary.supported_test_items;
                    result.evidence.extend(summary.records);
                }
                Err(err @ PluginError::Discovery { .. }) => {
                    return Err(err);
                }
                Err(err) => {
                    result
                        .diagnostics
                        .push(recoverable_plugin_diagnostic(file, &err));
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
        }

        if supported_test_items == 0 {
            if let Some(err) = first_error {
                return Err(err);
            }
            return Err(PluginError::Discovery {
                plugin: PLUGIN_NAME.to_string(),
                message: "zero supported Rust test items were found in the discovery scope; supported forms include #[test], #[tokio::test], supported proptest wrappers, and snapshot-oriented tests".to_string(),
                details: Some(Box::new(PluginErrorDetails {
                    code: Some("zero_supported_test_items".to_string()),
                    suggestion: Some(
                        "Annotate a supported Rust test with `#[verifies(\"doc#criterion\")]` or add criterion-nested `<VerifiedBy ... />` evidence.".to_string(),
                    ),
                    ..PluginErrorDetails::default()
                })),
            });
        }

        Ok(result)
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
        let id = EvidenceId::new(self.next_id);
        self.next_id += 1;
        id
    }
}

// ---------------------------------------------------------------------------
// File-level discovery
// ---------------------------------------------------------------------------

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
    targets: VerificationTargets,
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
        provenance: vec![PluginProvenance::RustAttribute {
            attribute_span: source_location,
        }],
        metadata,
    }
}

fn invalid_verifies_ref(span: proc_macro2::Span, value: &str) -> VerifiesParseError {
    let message = if value.is_empty() {
        "empty verifies reference".to_string()
    } else {
        format!(
            "invalid verifies reference `{value}`; use a full criterion ref like \
             `doc-id#criterion-id`",
        )
    };

    VerifiesParseError { message, span }
}

/// Map a `VerifiesParseError` into a `PluginError::Discovery` with location info.
fn parse_error_to_plugin_error(path: &Path, error: &VerifiesParseError) -> PluginError {
    PluginError::Discovery {
        plugin: PLUGIN_NAME.to_string(),
        message: error.message.clone(),
        details: Some(Box::new(PluginErrorDetails {
            path: Some(path.to_path_buf()),
            line: Some(error.span.start().line),
            column: Some(error.span.start().column + 1),
            code: Some("invalid_verifies_attribute".to_string()),
            suggestion: Some(suggestion_for_verifies_parse_error(&error.message)),
        })),
    }
}

fn suggestion_for_verifies_parse_error(message: &str) -> String {
    if message.contains("string literal ref") {
        "Use `#[verifies(\"doc#criterion\")]` with one or more string literal refs.".to_string()
    } else if message.contains("invalid verifies reference")
        || message.contains("empty verifies reference")
    {
        "Use a full criterion ref like `doc-id#criterion-id`.".to_string()
    } else {
        "Check the verifies attribute syntax and use a full criterion ref.".to_string()
    }
}

fn recoverable_plugin_diagnostic(path: &Path, error: &PluginError) -> PluginDiagnostic {
    let message = match error {
        PluginError::ParseFailure { message, .. } => {
            format!("skipping due to parse failure: {message}")
        }
        PluginError::Io { source, .. } => {
            format!("skipping due to I/O error: {source}")
        }
        PluginError::Discovery { message, .. } => message.clone(),
    };

    PluginDiagnostic::warning_for_path(path.to_path_buf(), message)
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
/// Accumulates targets from all `#[verifies(...)]` attributes on the item,
/// so multiple stacked attributes are supported.
fn extract_verifies_targets(
    attrs: &[syn::Attribute],
) -> Result<Option<(VerificationTargets, proc_macro2::Span)>, VerifiesParseError> {
    let mut all_targets = BTreeSet::new();
    let mut first_span = None;

    for attr in attrs {
        let path = attr.path();
        let is_verifies = path.is_ident("verifies")
            || (path.segments.len() == 2
                && path.segments[0].ident == "supersigil_rust"
                && path.segments[1].ident == "verifies");
        if is_verifies {
            let span = attr.span();
            first_span.get_or_insert(span);

            let syn::Meta::List(meta_list) = &attr.meta else {
                return Err(VerifiesParseError {
                    message: "`#[verifies(...)]` requires at least one string literal ref"
                        .to_string(),
                    span,
                });
            };

            let lit_strs: syn::punctuated::Punctuated<syn::LitStr, syn::token::Comma> = meta_list
                .parse_args_with(syn::punctuated::Punctuated::parse_terminated)
                .map_err(|_parse_err| VerifiesParseError {
                    message: "`#[verifies(...)]` expects string literal refs".to_string(),
                    span,
                })?;

            if lit_strs.is_empty() {
                return Err(VerifiesParseError {
                    message: "`#[verifies(...)]` requires at least one string literal ref"
                        .to_string(),
                    span,
                });
            }

            for lit_str in &lit_strs {
                let value = lit_str.value();
                let Some(verifiable_ref) = VerifiableRef::parse(&value) else {
                    return Err(invalid_verifies_ref(span, &value));
                };
                all_targets.insert(verifiable_ref);
            }
        }
    }

    if let Some(span) = first_span {
        let targets = VerificationTargets::new(all_targets)
            .expect("at least one valid criterion ref should yield a non-empty target set");
        Ok(Some((targets, span)))
    } else {
        Ok(None)
    }
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
        let mut segments = path.segments.iter();
        let first = segments.next();
        let second = segments.next();
        let third = segments.next();
        matches!(
            (first, second, third),
            (Some(a), Some(b), None) if a.ident == "tokio" && b.ident == "test"
        )
    })
}

fn has_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("test"))
}

fn body_contains_insta_snapshot(block: &syn::Block) -> bool {
    for stmt in &block.stmts {
        if let syn::Stmt::Macro(stmt_macro) = stmt
            && is_path(&stmt_macro.mac.path, &["insta", "assert_snapshot"])
        {
            return true;
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
            if is_path(&mac.path, &["insta", "assert_snapshot"]) {
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

/// Compare a `syn::Path`'s segments against expected identifier names
/// without allocating intermediate strings.
fn is_path(path: &syn::Path, expected: &[&str]) -> bool {
    path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(seg, name)| seg.ident == name)
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
mod tests;
