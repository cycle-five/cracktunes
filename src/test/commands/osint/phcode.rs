#[cfg(test)]
mod tests {
    use crate::commands::{fetch_country_by_calling_code, PhoneCodeData};

    #[test]
    fn test_fetch_country_by_calling_code_valid_code() {
        let phone_data = PhoneCodeData::load().unwrap();
        // Test the fetch_country_by_calling_code function with a valid calling code
        let result = fetch_country_by_calling_code(&phone_data, "1");

        println!("{:?}", result);

        // Assert that the result is Ok
        assert!(result.is_ok());
    }

    #[test]
    fn test_fetch_country_by_calling_code_invalid_code() {
        let phone_data = PhoneCodeData::load().unwrap();
        // Test the fetch_country_by_calling_code function with an invalid calling code
        let result = fetch_country_by_calling_code(&phone_data, "9999");

        // Assert that the result is Err
        assert!(result.is_err());
    }
}
