use crack_types::Error;
use sha1::{Digest, Sha1};
//use sha1::{digest::core_api::RtVariableCoreWrapper, Digest, Sha1};

/// Checks if a password has been exposed in data breaches using the `HaveIBeenPwned` API.
///
/// # Errors
///
/// Returns an error if:
/// - The API request fails
/// - The response cannot be parsed as text
pub async fn check_password_pwned(client: &reqwest::Client, password: &str) -> Result<bool, Error> {
    // Compute the SHA-1 hash of the password
    let hash = Sha1::digest(password.as_bytes());
    let hash_str = format!("{hash:X}");

    // Construct the URL for the API request
    let api_url = format!("https://api.pwnedpasswords.com/range/{}", &hash_str[0..5]);

    // Send the API request
    let response = client.get(&api_url).send().await?.text().await?;

    // Check if the full hash is in the response
    let pwned = response.contains(&hash_str[5..]);

    Ok(pwned)
}
