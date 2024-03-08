#[cfg(feature = "crack-osint")]
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_url_validator_valid_url() {
        assert!(url_validator("https://www.example.com"));
    }

    #[test]
    fn test_url_validator_invalid_url() {
        assert!(!url_validator("invalid_url"));
    }

    #[test]
    fn test_format_scan_result() {
        let scan_result = ScanResult {
            result_url: "https://urlscan.io/result/123456".to_string(),
        };

        let formatted_result = format_scan_result(&scan_result);
        assert_eq!(
            formatted_result,
            "Scan submitted successfully! Result URL: https://urlscan.io/result/123456"
        );
    }
}
