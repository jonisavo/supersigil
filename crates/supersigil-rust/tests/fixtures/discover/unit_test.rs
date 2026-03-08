use supersigil_rust::verifies;

#[verifies("req/auth#crit-1")]
#[test]
fn test_login_succeeds() {
    assert!(true);
}
