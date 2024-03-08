#[cfg(feature = "crack-osint")]
#[cfg(test)]
mod test {
    use crack_osint::check_password_pwned;

    #[tokio::test]
    async fn test_check_password_pwned() {
        // Test the check_password_pwned function with a known pwned password
        let result = check_password_pwned("password123").await;

        // Assert that the result is Ok and true
        assert_eq!(result.unwrap(), true);
    }
}
