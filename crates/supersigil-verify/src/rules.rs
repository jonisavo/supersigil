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
