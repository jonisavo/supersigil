use supersigil_rust::verifies;

#[verifies("req/auth#crit-1")]
#[verifies("req/security#crit-2")]
#[test]
fn test_with_stacked_verifies() {
    assert!(true);
}
