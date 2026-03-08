use supersigil_rust::verifies;

#[verifies(123)]
#[test]
fn test_bad_arg() {}

fn main() {}
