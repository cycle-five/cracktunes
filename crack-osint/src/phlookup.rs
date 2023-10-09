use crack_core::{
    messaging::message::CrackedMessage, utils::create_response_poise, Context, Error,
};
use reqwest::Client;
use serde::Deserialize;

/// Structure of the JSON response from the Numverify API
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct NumverifyResponse {
    valid: bool,
    number: String,
    local_format: String,
    international_format: String,
    country_prefix: String,
    country_code: String,
    country_name: String,
    location: String,
    carrier: String,
    line_type: String,
}

pub async fn fetch_phone_number_info(
    number: &str,
    country: &str,
) -> Result<NumverifyResponse, Error> {
    // Construct the URL for the API request
    let api_key = std::env::var("NUMVERIFY_API_KEY").ok();
    let api_url = format!(
        "http://apilayer.net/api/validate?access_key={}&number={}&country_code={}",
        api_key.unwrap_or_default(),
        number,
        country
    );

    // Send the API request
    let client = Client::new();
    let response: NumverifyResponse = client.get(&api_url).send().await?.json().await?;

    Ok(response)
}

/// Caller id lookup.
///
/// This command looks up the caller ID for a given phone number using the Numverify API.
/// The phone number and the country name code should be provided as arguments.
/// The command sends a response with the caller ID information or an error message if the lookup fails.
#[poise::command(hide_in_help, prefix_command)]
pub async fn phlookup(ctx: Context<'_>, number: String, country: String) -> Result<(), Error> {
    let response = fetch_phone_number_info(&number, &country).await?;
    let message = if response.valid {
        CrackedMessage::PhoneNumberInfo(format!("{:?}", response))
    } else {
        CrackedMessage::PhoneNumberInfoError
    };

    create_response_poise(ctx, message).await?;

    Ok(())
}
