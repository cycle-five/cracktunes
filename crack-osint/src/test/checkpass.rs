#[cfg(test)]
mod test {
    use crate::check_password_pwned;

    #[tokio::test]
    async fn test_check_password_pwned() {
        // Test the check_password_pwned function with a known pwned password
        let result = check_password_pwned("password123").await;

        // Assert that the result is Ok and true
        assert_eq!(result.unwrap(), true);
    }
}
