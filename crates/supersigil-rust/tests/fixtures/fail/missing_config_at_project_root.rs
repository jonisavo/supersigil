use supersigil_rust::verifies;

// When SUPERSIGIL_PROJECT_ROOT points to a directory without supersigil.toml,
// the macro should emit a compile-time error.
#[verifies("any/doc#any-criterion")]
#[test]
fn test_should_fail() {}

fn main() {}
