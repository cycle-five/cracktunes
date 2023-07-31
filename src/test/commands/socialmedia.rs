#[cfg(test)]
mod tests {
    use crate::commands::socialmedia::fetch_social_media_info;

    #[tokio::test]
    async fn test_fetch_social_media_info_valid_email() {
        // Test the fetch_social_media_info function with a valid email
        let result = fetch_social_media_info("example@gmail.com").await;

        // Assert that the result is Ok
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_social_media_info_invalid_email() {
        // Test the fetch_social_media_info function with an invalid email
        let result = fetch_social_media_info("not an email").await;

        // Assert that the result is Err
        assert!(result.is_err());
    }
}
