use crate::{send_response_poise, Context, CrackedMessage, Error};
use reqwest::Client;
use sha1::{Digest, Sha1};

pub async fn check_password_pwned(password: &str) -> Result<bool, Error> {
    // Compute the SHA-1 hash of the password
    let hash = Sha1::digest(password.as_bytes());
    let hash_str = format!("{:X}", hash);

    // Construct the URL for the API request
    let api_url = format!("https://api.pwnedpasswords.com/range/{}", &hash_str[0..5]);

    // Send the API request
    let client = Client::new();
    let response = client.get(&api_url).send().await?.text().await?;

    // Check if the full hash is in the response
    let pwned = response.contains(&hash_str[5..]);

    Ok(pwned)
}

/// Check if a password has been pwned.
#[poise::command(prefix_command, hide_in_help)]
pub async fn checkpass(ctx: Context<'_>, password: String) -> Result<(), Error> {
    let pwned = check_password_pwned(&password).await?;
    let message = if pwned {
        CrackedMessage::PasswordPwned
    } else {
        CrackedMessage::PasswordSafe
    };

    send_reply(ctx, message).await?;

    Ok(())
}
