//! Proc-macro crate for the `supersigil-rust` ecosystem plugin.
//!
//! Provides the `#[verifies(...)]` attribute macro that links Rust test
//! functions to supersigil specification criteria. This crate is not intended
//! to be depended on directly -- consumers should use `supersigil-rust`, which
//! re-exports the macro.

use std::cell::RefCell;
use std::collections::BTreeSet;
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
    config_mtime: SystemTime,
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

/// Discover spec files by expanding glob patterns relative to `base_dir`.
fn discover_spec_files(globs: &[String], base_dir: &Path) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    for pattern in globs {
        let full_pattern = base_dir.join(pattern);
        let pattern_str = full_pattern.to_string_lossy();
        if let Ok(entries) = glob::glob(pattern_str.as_ref()) {
            for entry in entries.flatten() {
                paths.insert(entry);
            }
        }
    }
    paths.into_iter().collect()
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

fn get_or_build_graph(
    project_root: &Path,
) -> Result<Option<Rc<supersigil_core::DocumentGraph>>, GraphErrors> {
    let config_path = project_root.join("supersigil.toml");
    let current_mtime = std::fs::metadata(&config_path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);

    // Check the thread-local cache.
    let cached: Option<Result<Rc<supersigil_core::DocumentGraph>, GraphErrors>> =
        GRAPH_CACHE.with(|cache| {
            let borrow = cache.borrow();
            if let Some(ref cached) = *borrow
                && cached.config_mtime == current_mtime
            {
                return Some(Ok(Rc::clone(&cached.graph)));
            }
            None
        });

    if let Some(result) = cached {
        return result.map(Some);
    }

    // Cache miss — rebuild.
    let config = match supersigil_core::load_config(&config_path) {
        Ok(c) => c,
        Err(errs) => {
            let detail: Vec<String> = errs.iter().map(std::string::ToString::to_string).collect();
            return Err(vec![(
                None,
                format!(
                    "supersigil: failed to load config at \"{}\": {}",
                    config_path.display(),
                    detail.join("; ")
                ),
            )]);
        }
    };

    if !should_validate(&config) {
        return Ok(None);
    }

    let globs: Vec<String> = if let Some(paths) = &config.paths {
        paths.clone()
    } else {
        match resolve_multi_project_globs(&config, project_root) {
            Ok(g) => g,
            Err(msg) => return Err(vec![(None, msg)]),
        }
    };

    let spec_files = discover_spec_files(&globs, project_root);
    let component_defs = supersigil_core::ComponentDefs::merge(
        supersigil_core::ComponentDefs::defaults(),
        config.components.clone(),
    )
    .map_err(|errs| {
        let msgs: Vec<String> = errs.iter().map(ToString::to_string).collect();
        vec![(
            None,
            format!(
                "supersigil: invalid component definitions: {}",
                msgs.join("; ")
            ),
        )]
    })?;

    let mut documents = Vec::new();
    let mut parse_errors: Vec<String> = Vec::new();
    for file in &spec_files {
        match supersigil_parser::parse_file(file, &component_defs) {
            Ok(supersigil_core::ParseResult::Document(doc)) => documents.push(doc),
            Ok(supersigil_core::ParseResult::NotSupersigil(_)) => {}
            Err(errs) => {
                let detail: Vec<String> =
                    errs.iter().map(std::string::ToString::to_string).collect();
                parse_errors.push(format!("{}: {}", file.display(), detail.join("; ")));
            }
        }
    }
    if !parse_errors.is_empty() {
        return Err(vec![(
            None,
            format!(
                "supersigil: failed to parse spec files: {}",
                parse_errors.join("; ")
            ),
        )]);
    }

    let graph = match supersigil_core::build_graph(documents, &config) {
        Ok(g) => g,
        Err(errs) => {
            let detail: Vec<String> = errs.iter().map(std::string::ToString::to_string).collect();
            return Err(vec![(
                None,
                format!(
                    "supersigil: failed to build document graph: {}",
                    detail.join("; ")
                ),
            )]);
        }
    };

    // Store in cache.
    let graph = Rc::new(graph);
    GRAPH_CACHE.with(|cache| {
        *cache.borrow_mut() = Some(CachedGraph {
            graph: Rc::clone(&graph),
            config_mtime: current_mtime,
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

/// Resolve spec file globs in multi-project mode.
///
/// Uses `CARGO_MANIFEST_DIR` and the config's `ecosystem.rust.project_scope`
/// entries (or path-based inference) to determine the applicable project.
///
/// NOTE: This logic mirrors `supersigil_rust::scope::resolve_scope()`.
/// The proc-macro cannot depend on `supersigil-rust` (reverse dependency),
/// so both implementations must be kept in sync.
fn resolve_multi_project_globs(
    config: &supersigil_core::Config,
    project_root: &Path,
) -> Result<Vec<String>, String> {
    let projects = config.projects.as_ref().ok_or_else(|| {
        "supersigil: multi-project mode detected but no projects configured".to_string()
    })?;

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").map_err(|_env_err| {
        "supersigil: multi-project mode requires CARGO_MANIFEST_DIR to be set".to_string()
    })?;
    let manifest_path = PathBuf::from(&manifest_dir);
    let relative = manifest_path
        .strip_prefix(project_root)
        .unwrap_or(&manifest_path);

    // Try explicit rust project_scope entries first.
    if let Some(rust_config) = &config.ecosystem.rust
        && !rust_config.project_scope.is_empty()
    {
        let matched = rust_config
            .project_scope
            .iter()
            .filter(|scope| relative.starts_with(&scope.manifest_dir_prefix))
            .max_by_key(|scope| scope.manifest_dir_prefix.len());

        return match matched {
            Some(scope) => {
                let project_config = projects.get(&scope.project).ok_or_else(|| {
                    format!(
                        "supersigil: project_scope maps to project \"{}\" \
                         which is not defined in [projects]",
                        scope.project
                    )
                })?;
                Ok(project_config.paths.clone())
            }
            None => Err(format!(
                "supersigil: no project_scope prefix matched manifest dir \"{}\" \
                 (relative: \"{}\")",
                manifest_dir,
                relative.display()
            )),
        };
    }

    // Path-based inference: check if manifest dir path components contain
    // a project name.
    let mut candidates: Vec<&str> = projects
        .keys()
        .filter(|name| {
            relative
                .components()
                .any(|c| c.as_os_str() == name.as_str())
        })
        .map(String::as_str)
        .collect();
    candidates.sort_unstable();

    match candidates.len() {
        1 => {
            let project_name = candidates[0];
            let project_config = &projects[project_name];
            Ok(project_config.paths.clone())
        }
        0 => Err(format!(
            "supersigil: multi-project mode but no project matched \
             manifest dir \"{}\" (relative: \"{}\"); configure \
             [ecosystem.rust.project_scope] to resolve this",
            manifest_dir,
            relative.display()
        )),
        _ => Err(format!(
            "supersigil: ambiguous project for manifest dir \"{}\" \
             (relative: \"{}\"): candidates {:?}; configure \
             [ecosystem.rust.project_scope] to resolve this",
            manifest_dir,
            relative.display(),
            candidates
        )),
    }
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
        match expr {
            Expr::Lit(expr_lit) => match &expr_lit.lit {
                Lit::Str(s) => {
                    let ref_string = s.value();
                    if let Err(err) = validate_ref_shape(&ref_string, s.span()) {
                        return err.to_compile_error().into();
                    }

                    ref_strings.push(ref_string);
                    ref_spans.push(s.span());
                }
                other => {
                    let err = syn::Error::new_spanned(
                        other,
                        format!(
                            "expected a string literal criterion reference, found `{}`",
                            quote!(#other)
                        ),
                    );
                    return err.to_compile_error().into();
                }
            },
            other => {
                let err = syn::Error::new_spanned(
                    other,
                    format!(
                        "expected a string literal criterion reference, found `{}`",
                        quote!(#other)
                    ),
                );
                return err.to_compile_error().into();
            }
        }
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
