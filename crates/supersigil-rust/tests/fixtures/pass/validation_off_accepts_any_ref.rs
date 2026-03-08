use supersigil_rust::verifies;

// When validation policy is "off", syntactically valid refs should compile
// without graph loading or ref resolution.
#[verifies("any/doc#any-criterion")]
#[test]
fn test_with_arbitrary_ref() {
    assert!(true);
}

fn main() {}
