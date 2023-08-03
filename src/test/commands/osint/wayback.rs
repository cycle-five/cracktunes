#[cfg(test)]
mod tests {
    use crate::commands::wayback::fetch_wayback_snapshot;

    #[tokio::test]
    async fn test_wayback_valid_url() {
        // Mock the Context to be used in the test

        // Test the wayback function with a valid URL
        let result = fetch_wayback_snapshot("https://www.openai.com").await;

        // Assert that the result is Ok
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wayback_invalid_url() {
        // Mock the Context to be used in the test

        // Test the wayback function with an invalid URL
        let result = fetch_wayback_snapshot("not a url").await;

        // Assert that the result is Err
        assert!(result.is_err());
    }
}
