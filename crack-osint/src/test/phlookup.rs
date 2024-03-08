#[cfg(feature = "crack-osint")]
#[cfg(test)]
mod test {
    use crack_osint::fetch_phone_number_info;

    #[tokio::test]
    #[ignore]
    async fn test_fetch_phone_number_info_valid_number() {
        // Test the fetch_phone_number_info function with a valid phone number and country
        let result = fetch_phone_number_info("14155552671", "US").await;

        // Assert that the result is Ok
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_fetch_phone_number_info_invalid_number() {
        // Test the fetch_phone_number_info function with an invalid phone number and country
        let result = fetch_phone_number_info("123", "ZZ").await;

        // Assert that the result is Err
        assert!(result.is_err());
    }
}
