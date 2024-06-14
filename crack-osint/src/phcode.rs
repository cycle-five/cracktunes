use crack_core::Error;
use crack_core::{
    errors::CrackedError, messaging::message::CrackedMessage, utils::send_reply, Context,
    PhoneCodeData,
};

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

    send_reply(ctx, CrackedMessage::CountryName(country_name)).await?;

    Ok(())
}
