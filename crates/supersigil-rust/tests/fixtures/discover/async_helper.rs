use supersigil_rust::verifies;

/// An async helper function with `#[verifies]` but NO `#[test]` attribute.
/// This should NOT produce evidence — only test functions count.
#[verifies("req/auth#crit-1")]
async fn setup_auth_context() {
    // Helper, not a test.
}
