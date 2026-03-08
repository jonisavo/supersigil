use supersigil_rust::verifies;

#[verifies("req/api#crit-1")]
#[tokio::test]
async fn test_api_call() {
    assert!(true);
}
