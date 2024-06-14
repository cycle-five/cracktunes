use std::fmt::{self, Display, Formatter};

use crack_core::{messaging::message::CrackedMessage, utils::send_reply, Context, Error};
use reqwest::Url;
use serde::Deserialize;

/// Structure of the JSON response from the hypothetical API
#[derive(Debug, Deserialize)]
pub struct SocialMediaResponse {
    name: String,
    #[serde(rename = "rateLimit")]
    rate_limit: bool,
    exists: bool,
    emailrecovery: Option<String>,
    #[serde(rename = "phoneNumber")]
    phone_number: Option<String>,
    others: Option<String>,
}

impl Display for SocialMediaResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Name: {}\nRate Limit: {}\nExists: {}\nEmail Recovery: {}\nPhone Number: {}\nOthers: {}",
            self.name,
            self.rate_limit,
            self.exists,
            self.emailrecovery.clone().unwrap_or_else(|| "None".to_string()),
            self.phone_number.clone().unwrap_or_else(|| "None".to_string()),
            self.others.clone().unwrap_or_else(|| "None".to_string())
        )
    }
}

pub async fn fetch_social_media_info(email: &str) -> Result<SocialMediaResponse, Error> {
    // Validate the email
    let _ = Url::parse(email)?;

    // Construct the URL for the hypothetical API request
    let api_url = format!("http://hypothetical-api.com/search?email={}", email);

    let client = reqwest::ClientBuild::new().use_rustls_tls().build()?;
    // Send the API request
    let response: SocialMediaResponse = client.get(&api_url).send().await?.json().await?;

    Ok(response)
}

/// Search for a given email address on social media.
#[poise::command(prefix_command, hide_in_help)]
pub async fn socialmedia(
    ctx: Context<'_>,
    #[description = "email to search on social media"] email: String,
) -> Result<(), Error> {
    match fetch_social_media_info(&email).await {
        Ok(response) => {
            // Send the response as the command's response
            send_reply(
                ctx,
                CrackedMessage::SocialMediaResponse {
                    response: format!("{:?}", response),
                },
            )
            .await?;
            Ok(())
        },
        Err(e) => Err(e),
    }
}
