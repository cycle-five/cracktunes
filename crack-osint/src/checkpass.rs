use crack_types::Error;
use sha1::{digest::core_api::RtVariableCoreWrapper, Digest, Sha1};

pub async fn check_password_pwned(client: &reqwest::Client, password: &str) -> Result<bool, Error> {
    // Compute the SHA-1 hash of the password
    let hash = Sha1::digest(password.as_bytes());
    let hash_str = format!("{:X}", hash);

    // Construct the URL for the API request
    let api_url = format!("https://api.pwnedpasswords.com/range/{}", &hash_str[0..5]);

    // Send the API request
    let response = client.get(&api_url).send().await?.text().await?;

    // Check if the full hash is in the response
    let pwned = response.contains(&hash_str[5..]);

    Ok(pwned)
}
