use supersigil_rust::verifies;

// When a spec file in the project has a parse error, the macro should
// emit a compile-time diagnostic instead of silently skipping validation.
#[verifies("broken/req#crit-1")]
#[test]
fn test_should_fail() {}

fn main() {}
