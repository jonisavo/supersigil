use supersigil_rust::verifies;

// This ref points to a criterion that does not exist in any spec.
// When validation is enabled, this should fail to compile.
#[verifies("nonexistent/doc#missing-criterion")]
#[test]
fn test_stale_ref() {}

fn main() {}
