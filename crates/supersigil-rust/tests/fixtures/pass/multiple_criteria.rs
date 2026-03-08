use supersigil_rust::verifies;

#[verifies("req/auth#crit-1", "req/auth#crit-2")]
#[test]
fn test_full_auth_flow() {
    assert!(true);
}

fn main() {}
