use supersigil_rust::verifies;

#[cfg(test)]
mod tests {
    use super::*;

    #[verifies("req/auth#crit-1")]
    #[test]
    fn test_inside_mod() {
        assert!(true);
    }

    #[verifies("req/auth#crit-2")]
    #[tokio::test]
    async fn test_async_inside_mod() {
        assert!(true);
    }
}
