//! Proc-macro crate for the `supersigil-rust` ecosystem plugin.
//!
//! Provides the `#[verifies(...)]` attribute macro that links Rust test
//! functions to supersigil specification criteria. This crate is not intended
//! to be depended on directly -- consumers should use `supersigil-rust`, which
//! re-exports the macro.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Expr, ItemFn, Lit, Token};

// ---------------------------------------------------------------------------
// Compile-time graph cache
// ---------------------------------------------------------------------------

/// Cached document graph for compile-time validation. The proc macro runs
/// in a single compiler process, so we cache the graph in a `thread_local!`
/// to avoid re-reading config, re-expanding globs, re-parsing spec files,
/// and re-building the graph for every `#[verifies]` invocation.
struct CachedGraph {
    graph: Rc<supersigil_core::DocumentGraph>,
    input_fingerprint: Vec<InputFingerprintEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InputFingerprintEntry {
    path: PathBuf,
    modified: SystemTime,
    len: u64,
}

thread_local! {
    static GRAPH_CACHE: RefCell<Option<CachedGraph>> = const { RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Attribute argument parsing
// ---------------------------------------------------------------------------

/// Parsed arguments for `#[verifies("ref1", "ref2", ...)]`.
struct VerifiesArgs {
    refs: Punctuated<Expr, Token![,]>,
}

impl Parse for VerifiesArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let refs = Punctuated::parse_terminated(input)?;
        Ok(Self { refs })
    }
}

// ---------------------------------------------------------------------------
// Graph validation
// ---------------------------------------------------------------------------

/// Walk up from `start` looking for `supersigil.toml`.
fn find_config(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join("supersigil.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn fingerprint_inputs(paths: &[PathBuf]) -> Vec<InputFingerprintEntry> {
    paths
        .iter()
        .map(|path| match std::fs::metadata(path) {
            Ok(metadata) => InputFingerprintEntry {
                path: path.clone(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                len: metadata.len(),
            },
            Err(_) => InputFingerprintEntry {
                path: path.clone(),
                modified: SystemTime::UNIX_EPOCH,
                len: 0,
            },
        })
        .collect()
}

/// Determine the project root path: either from `SUPERSIGIL_PROJECT_ROOT`
/// env var or by walking up from `CARGO_MANIFEST_DIR`.
///
/// Returns:
/// - `Ok(None)` — no project root found or explicitly disabled; skip validation.
/// - `Ok(Some(path))` — found project root; proceed with validation.
/// - `Err(msg)` — explicit project root is invalid; emit a compile error.
fn resolve_project_root() -> Result<Option<PathBuf>, String> {
    // First check explicit env var.
    if let Ok(root) = std::env::var("SUPERSIGIL_PROJECT_ROOT") {
        // Empty value means "explicitly disabled".
        if root.is_empty() {
            return Ok(None);
        }
        let p = PathBuf::from(&root);
        if p.join("supersigil.toml").is_file() {
            return Ok(Some(p));
        }
        // Explicit root set but config not found — this is an error.
        return Err(format!(
            "SUPERSIGIL_PROJECT_ROOT is set to \"{root}\" but no supersigil.toml \
             was found at that path"
        ));
    }

    // Walk up from CARGO_MANIFEST_DIR.
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return Ok(None);
    };
    Ok(find_config(Path::new(&manifest_dir)).and_then(|p| p.parent().map(Path::to_path_buf)))
}

/// Check whether graph validation should run given the loaded config.
fn should_validate(config: &supersigil_core::Config) -> bool {
    use supersigil_core::RustValidationPolicy;

    let policy = config
        .ecosystem
        .rust
        .as_ref()
        .map_or(RustValidationPolicy::Dev, |r| r.validation);

    match policy {
        RustValidationPolicy::Off => false,
        RustValidationPolicy::All => true,
        RustValidationPolicy::Dev => {
            // Skip validation in release builds.
            let profile = std::env::var("PROFILE").unwrap_or_default();
            profile != "release"
        }
    }
}

/// Build (or retrieve from cache) the document graph for the given project root.
///
/// Returns `Ok(None)` if validation should be skipped, `Ok(Some(graph))` on
/// success, or `Err` with an error message.
type GraphErrors = Vec<(Option<String>, String)>;

fn graph_error(context: &str, errors: &[impl std::fmt::Display]) -> GraphErrors {
    let detail = errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ");
    vec![(None, format!("supersigil: {context}: {detail}"))]
}

fn get_or_build_graph(
    project_root: &Path,
) -> Result<Option<Rc<supersigil_core::DocumentGraph>>, GraphErrors> {
    let config_path = project_root.join("supersigil.toml");
    let config = match supersigil_core::load_config(&config_path) {
        Ok(c) => c,
        Err(errs) => {
            return Err(graph_error(
                &format!("failed to load config at \"{}\"", config_path.display()),
                &errs,
            ));
        }
    };

    if !should_validate(&config) {
        return Ok(None);
    }

    let inputs = supersigil_core::resolve_workspace_validation_inputs(&config, project_root)
        .map_err(|err| vec![(None, format!("supersigil: {err}"))])?;
    let current_fingerprint = fingerprint_inputs(&inputs.all_paths());

    // Check the thread-local cache after resolving the full validation inputs.
    let cached: Option<Result<Rc<supersigil_core::DocumentGraph>, GraphErrors>> =
        GRAPH_CACHE.with(|cache| {
            let borrow = cache.borrow();
            if let Some(ref cached) = *borrow
                && cached.input_fingerprint == current_fingerprint
            {
                return Some(Ok(Rc::clone(&cached.graph)));
            }
            None
        });

    if let Some(result) = cached {
        return result.map(Some);
    }

    let component_defs = supersigil_core::ComponentDefs::merge(
        supersigil_core::ComponentDefs::defaults(),
        config.components.clone(),
    )
    .map_err(|errs| graph_error("invalid component definitions", &errs))?;

    let mut documents = Vec::new();
    let mut parse_errors: Vec<String> = Vec::new();
    for file in &inputs.spec_files {
        match supersigil_parser::parse_file(file, &component_defs) {
            Ok(supersigil_core::ParseResult::Document(doc)) => documents.push(doc),
            Ok(supersigil_core::ParseResult::NotSupersigil(_)) => {}
            Err(errs) => {
                let detail = errs
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                parse_errors.push(format!("{}: {detail}", file.display()));
            }
        }
    }
    if !parse_errors.is_empty() {
        return Err(graph_error("failed to parse spec files", &parse_errors));
    }

    let graph = match supersigil_core::build_graph(documents, &config) {
        Ok(g) => g,
        Err(errs) => return Err(graph_error("failed to build document graph", &errs)),
    };

    // Store in cache.
    let graph = Rc::new(graph);
    GRAPH_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(CachedGraph {
            graph: Rc::clone(&graph),
            input_fingerprint: current_fingerprint,
        });
    });

    Ok(Some(graph))
}

/// Validate criterion refs against the document graph.
///
/// Returns a list of error messages. Each entry is either:
/// - `(Some(ref_string), message)` — tied to a specific ref (use its span)
/// - `(None, message)` — a general error (use `call_site` span)
fn validate_refs(refs: &[String], project_root: &Path) -> Vec<(Option<String>, String)> {
    let graph = match get_or_build_graph(project_root) {
        Ok(Some(g)) => g,
        Ok(None) => return Vec::new(),
        Err(errors) => return errors,
    };

    // Check each ref.
    let mut errors = Vec::new();
    for ref_str in refs {
        let Some((doc_id, fragment)) = ref_str.split_once('#') else {
            continue;
        };

        if graph.component(doc_id, fragment).is_none() {
            errors.push((
                Some(ref_str.clone()),
                format!(
                    "unresolved criterion reference \"{ref_str}\": \
                     no matching criterion found in the specification graph"
                ),
            ));
        }
    }
    errors
}

fn validate_ref_shape(ref_str: &str, span: proc_macro2::Span) -> syn::Result<()> {
    let Some((doc_id, criterion_id)) = ref_str.split_once('#') else {
        return Err(syn::Error::new(
            span,
            format!(
                "invalid criterion reference \"{ref_str}\": expected `document-id#criterion-id`"
            ),
        ));
    };

    if doc_id.is_empty() || criterion_id.is_empty() || criterion_id.contains('#') {
        return Err(syn::Error::new(
            span,
            format!(
                "invalid criterion reference \"{ref_str}\": expected `document-id#criterion-id`"
            ),
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Proc-macro entry point
// ---------------------------------------------------------------------------

/// Attribute macro that marks a test function as verifying one or more
/// specification criteria.
///
/// # Usage
///
/// ```ignore
/// #[supersigil::verifies("req/auth#crit-1")]
/// #[test]
/// fn test_login_succeeds() {
///     // ...
/// }
/// ```
///
/// The macro validates:
/// 1. **Syntax**: at least one string-literal argument.
/// 2. **Shape**: each ref must use the `document-id#criterion-id` form.
/// 3. **Graph** (when enabled): each criterion ref resolves in the `DocumentGraph`.
///
/// The annotated item is emitted unchanged — no runtime behaviour is added.
#[proc_macro_attribute]
pub fn verifies(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse attribute arguments.
    let args: VerifiesArgs = match syn::parse(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error().into(),
    };

    // Must have at least one argument.
    if args.refs.is_empty() {
        let err = syn::Error::new(
            proc_macro2::Span::call_site(),
            "`#[verifies(...)]` requires at least one criterion reference string",
        );
        return err.to_compile_error().into();
    }

    // Each argument must be a string literal.
    let mut ref_strings: Vec<String> = Vec::new();
    let mut ref_spans: Vec<proc_macro2::Span> = Vec::new();
    for expr in &args.refs {
        let Expr::Lit(syn::ExprLit {
            lit: Lit::Str(s), ..
        }) = expr
        else {
            let err = syn::Error::new_spanned(
                expr,
                format!(
                    "expected a string literal criterion reference, found `{}`",
                    quote!(#expr)
                ),
            );
            return err.to_compile_error().into();
        };

        let ref_string = s.value();
        if let Err(err) = validate_ref_shape(&ref_string, s.span()) {
            return err.to_compile_error().into();
        }
        ref_strings.push(ref_string);
        ref_spans.push(s.span());
    }

    // The annotated item must be a function.
    let item_clone: proc_macro2::TokenStream = item.clone().into();
    if syn::parse2::<ItemFn>(item_clone).is_err() {
        let err = syn::Error::new(
            proc_macro2::Span::call_site(),
            "`#[verifies(...)]` can only be applied to functions",
        );
        return err.to_compile_error().into();
    }

    // Optional graph validation.
    match resolve_project_root() {
        Ok(Some(project_root)) => {
            let errors = validate_refs(&ref_strings, &project_root);
            if !errors.is_empty() {
                let mut combined: Option<syn::Error> = None;
                for (ref_str, message) in &errors {
                    let span = ref_str
                        .as_ref()
                        .and_then(|r| ref_strings.iter().position(|s| s == r))
                        .map_or_else(proc_macro2::Span::call_site, |idx| ref_spans[idx]);
                    let err = syn::Error::new(span, message);
                    match &mut combined {
                        None => combined = Some(err),
                        Some(existing) => existing.combine(err),
                    }
                }
                if let Some(combined) = combined {
                    return combined.to_compile_error().into();
                }
            }
        }
        Ok(None) => {
            // No project root found — skip validation.
        }
        Err(msg) => {
            let err = syn::Error::new(proc_macro2::Span::call_site(), msg);
            return err.to_compile_error().into();
        }
    }

    // Emit item unchanged.
    item
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn clear_graph_cache() {
        GRAPH_CACHE.with(|cache| {
            *cache.borrow_mut() = None;
        });
    }

    fn write_config(root: &Path) {
        fs::write(
            root.join("supersigil.toml"),
            "paths = [\"specs/**/*.mdx\"]\n",
        )
        .unwrap();
    }

    fn write_spec(root: &Path, criterion_id: &str) {
        fs::create_dir_all(root.join("specs")).unwrap();
        fs::write(
            root.join("specs/auth.mdx"),
            format!(
                "---\nsupersigil:\n  id: auth/req\n  type: requirements\n  status: approved\n---\n\n<AcceptanceCriteria>\n  <Criterion id=\"{criterion_id}\">\n    Must log in.\n  </Criterion>\n</AcceptanceCriteria>\n"
            ),
        )
        .unwrap();
    }

    fn config_with_policy(
        policy: supersigil_core::RustValidationPolicy,
    ) -> supersigil_core::Config {
        supersigil_core::Config {
            ecosystem: supersigil_core::EcosystemConfig {
                rust: Some(supersigil_core::RustEcosystemConfig {
                    validation: policy,
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn should_validate_off_skips() {
        let config = config_with_policy(supersigil_core::RustValidationPolicy::Off);
        assert!(!should_validate(&config), "policy=off must skip validation");
    }

    #[test]
    fn should_validate_all_always_validates() {
        let config = config_with_policy(supersigil_core::RustValidationPolicy::All);
        assert!(
            should_validate(&config),
            "policy=all must validate unconditionally"
        );
    }

    #[test]
    fn should_validate_dev_validates_in_debug() {
        // SAFETY: nextest runs each test in its own process.
        unsafe { std::env::set_var("PROFILE", "debug") };
        let config = config_with_policy(supersigil_core::RustValidationPolicy::Dev);
        assert!(
            should_validate(&config),
            "policy=dev must validate when PROFILE=debug"
        );
    }

    #[test]
    fn should_validate_dev_skips_in_release() {
        // SAFETY: nextest runs each test in its own process.
        unsafe { std::env::set_var("PROFILE", "release") };
        let config = config_with_policy(supersigil_core::RustValidationPolicy::Dev);
        assert!(
            !should_validate(&config),
            "policy=dev must skip validation when PROFILE=release"
        );
    }

    #[test]
    fn should_validate_default_is_dev() {
        // When no rust config is provided, the default policy is Dev.
        let config = supersigil_core::Config::default();
        // SAFETY: nextest runs each test in its own process.
        unsafe { std::env::set_var("PROFILE", "debug") };
        assert!(
            should_validate(&config),
            "default policy (dev) must validate in debug"
        );
        // SAFETY: nextest runs each test in its own process.
        unsafe { std::env::set_var("PROFILE", "release") };
        assert!(
            !should_validate(&config),
            "default policy (dev) must skip validation in release"
        );
    }

    #[test]
    fn validate_refs_rebuilds_graph_when_spec_file_changes() {
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path();
        write_config(project_root);
        write_spec(project_root, "ac-1");
        clear_graph_cache();

        let refs = vec!["auth/req#ac-1".to_string()];
        let first = validate_refs(&refs, project_root);
        assert!(first.is_empty(), "initial ref should resolve: {first:?}");

        write_spec(project_root, "criterion-two-longer-than-before");

        let second = validate_refs(&refs, project_root);
        assert!(
            second
                .iter()
                .any(|(_, message)| message.contains("unresolved criterion reference")),
            "changed spec should invalidate the cache and make the old ref fail: {second:?}",
        );
    }
}
