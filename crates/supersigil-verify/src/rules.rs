use supersigil_core::ExtractedComponent;

pub mod coverage;
pub mod status;
pub mod structural;
pub mod tests_rule;
pub mod tracked;

/// Recursively find all components with the given `name` in a component tree.
pub(crate) fn find_components<'a>(
    components: &'a [ExtractedComponent],
    name: &str,
) -> Vec<&'a ExtractedComponent> {
    let mut result = Vec::new();
    for c in components {
        if c.name == name {
            result.push(c);
        }
        result.extend(find_components(&c.children, name));
    }
    result
}

/// Returns `true` if any component (recursively) has the given `name`.
pub(crate) fn has_component(components: &[ExtractedComponent], name: &str) -> bool {
    components
        .iter()
        .any(|c| c.name == name || has_component(&c.children, name))
}

/// Collect all `VerifiedBy` components that are direct children of a
/// `Criterion` component.
///
/// Only criterion-nested `VerifiedBy` components produce coverage evidence.
/// Document-level placement is caught separately by
/// `structural::check_verified_by_placement`.
pub(crate) fn find_criterion_nested_verified_by(
    components: &[ExtractedComponent],
) -> Vec<&ExtractedComponent> {
    let mut result = Vec::new();
    collect_criterion_nested_verified_by(components, false, &mut result);
    result
}

fn collect_criterion_nested_verified_by<'a>(
    components: &'a [ExtractedComponent],
    inside_criterion: bool,
    result: &mut Vec<&'a ExtractedComponent>,
) {
    for c in components {
        if c.name == "VerifiedBy" && inside_criterion {
            result.push(c);
        }
        let child_inside_criterion = c.name == "Criterion";
        collect_criterion_nested_verified_by(&c.children, child_inside_criterion, result);
    }
}
