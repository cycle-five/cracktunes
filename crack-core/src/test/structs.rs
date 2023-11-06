#[cfg(test)]
mod test {
    #[test]
    fn test_phone_code_data() {
        use crate::PhoneCodeData;

        let data = PhoneCodeData::load().unwrap();
        let countries = data.get_countries_by_phone_code("1");
        assert!(countries.is_some());
        let countries = countries.unwrap();
        assert!(countries.contains(&"US".to_string()));
        assert!(countries.contains(&"CA".to_string()));
    }
}
