use crate::Error;
use crate::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::create_response_poise, Context,
};
use reqwest::blocking::get;
use std::collections::HashMap;
use std::fs;
use std::io::Write;

#[derive(Default, Debug, Clone)]
pub struct PhoneCodeData {
    #[allow(dead_code)]
    phone_codes: HashMap<String, String>,
    #[allow(dead_code)]
    country_names: HashMap<String, String>,
    country_by_phone_code: HashMap<String, Vec<String>>,
}

impl PhoneCodeData {
    pub fn load() -> Result<Self, CrackedError> {
        let phone_codes = load_data("data/phone.json", "http://country.io/phone.json")?;
        let country_names = load_data("data/names.json", "http://country.io/names.json")?;
        let country_by_phone_code = phone_codes
            .iter()
            .map(|(k, v)| (v.clone(), k.clone()))
            .fold(
                HashMap::new(),
                |mut acc: HashMap<String, Vec<String>>, (k, v)| {
                    acc.entry(k).or_default().push(v);
                    acc
                },
            );
        Ok(Self {
            phone_codes,
            country_names,
            country_by_phone_code,
        })
    }

    pub fn get_countries_by_phone_code(&self, phone_code: &str) -> Option<Vec<String>> {
        self.country_by_phone_code.get(phone_code).cloned()
    }
}

fn load_data(file_name: &str, url: &str) -> Result<HashMap<String, String>, CrackedError> {
    match fs::read_to_string(file_name) {
        Ok(contents) => {
            serde_json::from_str(&contents).map_err(|_| CrackedError::Other("Failed to parse file"))
        }
        Err(_) => download_and_parse(url, file_name),
    }
}

fn download_and_parse(url: &str, file_name: &str) -> Result<HashMap<String, String>, CrackedError> {
    let response = get(url).map_err(|_| CrackedError::Other("Failed to download"))?;
    let content = response
        .text()
        .map_err(|_| CrackedError::Other("Failed to read response"))?;

    // Save to local file
    let mut file =
        fs::File::create(file_name).map_err(|_| CrackedError::Other("Failed to create file"))?;
    file.write_all(content.as_bytes())
        .map_err(|_| CrackedError::Other("Failed to write file"))?;

    serde_json::from_str(&content).map_err(|_| CrackedError::Other("Failed to parse file"))
}

pub fn fetch_country_by_calling_code(
    phone_data: &PhoneCodeData,
    calling_code: &str,
) -> Result<String, CrackedError> {
    let country_name = phone_data
        .get_countries_by_phone_code(calling_code)
        .ok_or_else(|| CrackedError::Other("Invalid calling code"))?
        .join(", ")
        .clone();

    Ok(country_name)
}

/// Find the country of a calling code.
///
/// This command takes a calling code as an argument and fetches the associated countries.
#[poise::command(hide_in_help, prefix_command)]
pub async fn phcode(ctx: Context<'_>, calling_code: String) -> Result<(), Error> {
    let phone_data = ctx.data().phone_data.clone();
    let country_name = fetch_country_by_calling_code(&phone_data, &calling_code)?;

    create_response_poise(ctx, CrackedMessage::CountryName(country_name)).await?;

    Ok(())
}
