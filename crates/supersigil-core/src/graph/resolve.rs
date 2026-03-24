//! Ref resolution and task-implements resolution (pipeline stages 3–4).

use std::collections::HashMap;

use crate::{
    ComponentDefs, ExtractedComponent, SourcePosition, SpecDocument, split_list_attribute,
};

use super::error::GraphError;
use super::{EXAMPLE, ResolvedRef, TASK};

// ---------------------------------------------------------------------------
// Ref parsing
// ---------------------------------------------------------------------------

/// Parse a ref string like `"auth/req/login#valid-creds"` into
/// `(doc_id, Option<fragment>)`.
fn parse_ref(raw: &str) -> (String, Option<String>) {
    match raw.find('#') {
        Some(pos) => (raw[..pos].to_owned(), Some(raw[pos + 1..].to_owned())),
        None => (raw.to_owned(), None),
    }
}

// ---------------------------------------------------------------------------
// ResolveContext — shared read-only state for resolution
// ---------------------------------------------------------------------------

/// Read-only context shared across all resolution functions.
///
/// Bundles the immutable indexes and project metadata that every resolution
/// function needs, avoiding parameter sprawl.
pub(super) struct ResolveContext<'a> {
    pub(super) doc_index: &'a HashMap<String, SpecDocument>,
    pub(super) component_index: &'a HashMap<(String, String), ExtractedComponent>,
    pub(super) component_defs: &'a ComponentDefs,
    pub(super) doc_project: &'a HashMap<String, Option<String>>,
    pub(super) project_isolation: &'a HashMap<String, bool>,
}

// ---------------------------------------------------------------------------
// Shared ref validation
// ---------------------------------------------------------------------------

/// Outcome of validating a single parsed ref.
enum RefValidation {
    /// Ref is valid: `(target_doc_id, Option<fragment>)`.
    Valid(String, Option<String>),
    /// Ref produced an error (already pushed to the errors vec).
    Error,
}

/// Fragment validation mode for `validate_ref`.
#[derive(Clone, Copy)]
enum FragmentCheck<'a> {
    /// No constraint on the resolved component type (used by generic `refs`
    /// resolution when `target_component` is `None`).
    None,
    /// The resolved fragment must be a specific component type (used by
    /// `refs` resolution when `target_component` is `Some`).
    ExactComponent(&'a str),
    /// The resolved fragment must be a *verifiable* component (checked via
    /// `ComponentDefs`). Used by `Task.implements` resolution.
    Verifiable,
}

/// Validate a single parsed ref: check cross-project isolation, document
/// existence, and optionally fragment existence and component type.
///
/// If `require_fragment` is `true`, refs without a `#fragment` are rejected.
/// `fragment_check` controls how the resolved fragment's component type is
/// validated (see [`FragmentCheck`]).
///
/// Errors are pushed directly into `errors`; callers inspect the return value
/// to decide whether to record the resolved ref.
fn validate_ref(
    ctx: &ResolveContext<'_>,
    doc_id: &str,
    raw: &str,
    position: SourcePosition,
    require_fragment: bool,
    fragment_check: FragmentCheck<'_>,
    errors: &mut Vec<GraphError>,
) -> RefValidation {
    let (target_doc_id, fragment) = parse_ref(raw);

    // Fragment-required check (implements refs).
    if require_fragment && fragment.is_none() {
        errors.push(GraphError::BrokenRef {
            doc_id: doc_id.to_owned(),
            ref_str: raw.to_owned(),
            reason: "implements ref must include a #fragment targeting a verifiable component"
                .to_owned(),
            position,
        });
        return RefValidation::Error;
    }

    // Cross-project isolation check.
    if is_cross_project_violation(
        doc_id,
        &target_doc_id,
        ctx.doc_project,
        ctx.project_isolation,
    ) {
        errors.push(GraphError::BrokenRef {
            doc_id: doc_id.to_owned(),
            ref_str: raw.to_owned(),
            reason: format!(
                "cross-project reference from isolated project (source `{doc_id}` → target `{target_doc_id}`)"
            ),
            position,
        });
        return RefValidation::Error;
    }

    // Document existence check.
    if !ctx.doc_index.contains_key(&target_doc_id) {
        errors.push(GraphError::BrokenRef {
            doc_id: doc_id.to_owned(),
            ref_str: raw.to_owned(),
            reason: format!("document `{target_doc_id}` not found"),
            position,
        });
        return RefValidation::Error;
    }

    // Fragment existence and component type check.
    if let Some(ref frag) = fragment {
        let key = (target_doc_id.clone(), frag.clone());
        if let Some(comp) = ctx.component_index.get(&key) {
            match fragment_check {
                FragmentCheck::None => {}
                FragmentCheck::ExactComponent(expected) => {
                    if comp.name != expected {
                        errors.push(GraphError::BrokenRef {
                            doc_id: doc_id.to_owned(),
                            ref_str: raw.to_owned(),
                            reason: format!(
                                "fragment `{frag}` resolves to `{}` but expected `{expected}`",
                                comp.name
                            ),
                            position,
                        });
                        return RefValidation::Error;
                    }
                }
                FragmentCheck::Verifiable => {
                    let is_verifiable = ctx
                        .component_defs
                        .get(&comp.name)
                        .is_some_and(|def| def.verifiable);
                    if !is_verifiable {
                        errors.push(GraphError::BrokenRef {
                            doc_id: doc_id.to_owned(),
                            ref_str: raw.to_owned(),
                            reason: format!(
                                "fragment `{frag}` resolves to `{}` which is not a verifiable component",
                                comp.name
                            ),
                            position,
                        });
                        return RefValidation::Error;
                    }
                }
            }
        } else {
            errors.push(GraphError::BrokenRef {
                doc_id: doc_id.to_owned(),
                ref_str: raw.to_owned(),
                reason: format!("fragment `{frag}` not found in document `{target_doc_id}`"),
                position,
            });
            return RefValidation::Error;
        }
    }

    RefValidation::Valid(target_doc_id, fragment)
}

// ---------------------------------------------------------------------------
// Stage 3: Ref resolution
// ---------------------------------------------------------------------------

/// Resolve all `refs` attributes across all documents.
///
/// For each component with a `refs` attribute (determined by `ComponentDefs`),
/// splits the attribute value, parses each ref, and validates it against the
/// document and component indexes.
///
/// Returns the resolved refs map and any `BrokenRef` errors.
#[expect(clippy::type_complexity, reason = "return type is clear in context")]
pub(super) fn resolve_refs(
    ctx: &ResolveContext<'_>,
) -> (
    HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,
    Vec<GraphError>,
) {
    let mut errors = Vec::new();
    let mut resolved_refs: HashMap<(String, Vec<usize>), Vec<ResolvedRef>> = HashMap::new();

    for (doc_id, doc) in ctx.doc_index {
        resolve_components_recursive(
            ctx,
            doc_id,
            &doc.components,
            &[],
            &mut resolved_refs,
            &mut errors,
        );
    }

    (resolved_refs, errors)
}

/// Recursively walk components and resolve `refs` attributes.
fn resolve_components_recursive(
    ctx: &ResolveContext<'_>,
    doc_id: &str,
    components: &[ExtractedComponent],
    parent_path: &[usize],
    resolved_refs: &mut HashMap<(String, Vec<usize>), Vec<ResolvedRef>>,
    errors: &mut Vec<GraphError>,
) {
    for (idx, component) in components.iter().enumerate() {
        let mut component_path = parent_path.to_vec();
        component_path.push(idx);

        // Check if this component type has a `refs` attribute defined.
        if let Some(def) = ctx.component_defs.get(&component.name)
            && let Some(refs_value) = component.attributes.get("refs")
        {
            let fragment_check = match def.target_component.as_deref() {
                Some(tc) => FragmentCheck::ExactComponent(tc),
                None => FragmentCheck::None,
            };
            let resolved = resolve_single_refs_attribute(
                ctx,
                doc_id,
                refs_value,
                component.position,
                fragment_check,
                errors,
            );

            if !resolved.is_empty() {
                resolved_refs.insert((doc_id.to_owned(), component_path.clone()), resolved);
            }
        }

        // Example components may have `references` (informational edges) or
        // `verifies` (verification edges targeting verifiable components).
        // Both create reference edges for LSP navigation.
        if component.name == EXAMPLE {
            const EXAMPLE_REF_ATTRS: &[(&str, FragmentCheck<'_>)] = &[
                ("references", FragmentCheck::None),
                ("verifies", FragmentCheck::Verifiable),
            ];

            for &(attr, check) in EXAMPLE_REF_ATTRS {
                if let Some(value) = component.attributes.get(attr) {
                    let resolved = resolve_single_refs_attribute(
                        ctx,
                        doc_id,
                        value,
                        component.position,
                        check,
                        errors,
                    );

                    if !resolved.is_empty() {
                        resolved_refs
                            .entry((doc_id.to_owned(), component_path.clone()))
                            .or_default()
                            .extend(resolved);
                    }
                }
            }
        }

        // Recurse into children.
        if !component.children.is_empty() {
            resolve_components_recursive(
                ctx,
                doc_id,
                &component.children,
                &component_path,
                resolved_refs,
                errors,
            );
        }
    }
}

/// Resolve a single `refs` attribute value (comma-separated list of refs).
fn resolve_single_refs_attribute(
    ctx: &ResolveContext<'_>,
    doc_id: &str,
    refs_value: &str,
    position: SourcePosition,
    fragment_check: FragmentCheck<'_>,
    errors: &mut Vec<GraphError>,
) -> Vec<ResolvedRef> {
    let items = match split_list_attribute(refs_value) {
        Ok(items) => items,
        Err(e) => {
            errors.push(GraphError::BrokenRef {
                doc_id: doc_id.to_owned(),
                ref_str: refs_value.to_owned(),
                reason: e.message,
                position,
            });
            return Vec::new();
        }
    };

    let mut resolved = Vec::with_capacity(items.len());

    for raw in items {
        if let RefValidation::Valid(target_doc_id, fragment) =
            validate_ref(ctx, doc_id, raw, position, false, fragment_check, errors)
        {
            resolved.push(ResolvedRef {
                raw: raw.to_owned(),
                target_doc_id,
                fragment,
            });
        }
    }

    resolved
}

/// Check if a ref from `source_doc_id` to `target_doc_id` violates project
/// isolation rules.
///
/// Returns `true` if the source document is in an isolated project and the
/// target document is in a different project (or no project).
fn is_cross_project_violation(
    source_doc_id: &str,
    target_doc_id: &str,
    doc_project: &HashMap<String, Option<String>>,
    project_isolation: &HashMap<String, bool>,
) -> bool {
    let source_project = doc_project.get(source_doc_id).and_then(|p| p.as_deref());
    let target_project = doc_project.get(target_doc_id).and_then(|p| p.as_deref());

    // Only check isolation if the source belongs to a named project.
    let Some(src_proj) = source_project else {
        return false;
    };

    // Check if the source project is isolated.
    let isolated = project_isolation.get(src_proj).copied().unwrap_or(false);

    if !isolated {
        return false;
    }

    // Isolated: target must be in the same project.
    target_project != Some(src_proj)
}

// ---------------------------------------------------------------------------
// Stage 4: Task implements resolution
// ---------------------------------------------------------------------------

/// Resolve `implements` attributes on all `Task` components.
///
/// Each ref in an `implements` attribute MUST include a `#fragment` targeting
/// a verifiable component (one whose `ComponentDef` has `verifiable: true`).
/// A ref without a fragment, or one that does not resolve to a verifiable
/// component, produces a `BrokenRef` error.
///
/// Returns the resolved mappings and any errors.
#[expect(clippy::type_complexity, reason = "return type is clear in context")]
pub(super) fn resolve_task_implements(
    ctx: &ResolveContext<'_>,
) -> (
    HashMap<(String, String), Vec<(String, String)>>,
    Vec<GraphError>,
) {
    let mut errors = Vec::new();
    let mut task_implements: HashMap<(String, String), Vec<(String, String)>> = HashMap::new();

    for (doc_id, doc) in ctx.doc_index {
        resolve_task_implements_recursive(
            ctx,
            doc_id,
            &doc.components,
            &mut task_implements,
            &mut errors,
        );
    }

    (task_implements, errors)
}

/// Recursively walk components looking for `Task` components with `implements`.
fn resolve_task_implements_recursive(
    ctx: &ResolveContext<'_>,
    doc_id: &str,
    components: &[ExtractedComponent],
    task_implements: &mut HashMap<(String, String), Vec<(String, String)>>,
    errors: &mut Vec<GraphError>,
) {
    for component in components {
        if component.name == TASK
            && let Some(impl_value) = component.attributes.get("implements")
            && let Some(task_id) = component.attributes.get("id")
        {
            let resolved = resolve_single_implements_attribute(
                ctx,
                doc_id,
                impl_value,
                component.position,
                errors,
            );

            if !resolved.is_empty() {
                task_implements.insert((doc_id.to_owned(), task_id.clone()), resolved);
            }
        }

        // Recurse into children (nested tasks).
        if !component.children.is_empty() {
            resolve_task_implements_recursive(
                ctx,
                doc_id,
                &component.children,
                task_implements,
                errors,
            );
        }
    }
}

/// Resolve a single `implements` attribute value.
///
/// Each ref must have a `#fragment` targeting a verifiable component.
fn resolve_single_implements_attribute(
    ctx: &ResolveContext<'_>,
    doc_id: &str,
    impl_value: &str,
    position: SourcePosition,
    errors: &mut Vec<GraphError>,
) -> Vec<(String, String)> {
    let items = match split_list_attribute(impl_value) {
        Ok(items) => items,
        Err(e) => {
            errors.push(GraphError::BrokenRef {
                doc_id: doc_id.to_owned(),
                ref_str: impl_value.to_owned(),
                reason: e.message,
                position,
            });
            return Vec::new();
        }
    };

    let mut resolved = Vec::with_capacity(items.len());

    for raw in items {
        if let RefValidation::Valid(target_doc_id, Some(frag)) = validate_ref(
            ctx,
            doc_id,
            raw,
            position,
            true,
            FragmentCheck::Verifiable,
            errors,
        ) {
            resolved.push((target_doc_id, frag));
        }
    }

    resolved
}
