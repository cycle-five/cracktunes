use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;

/// Set the prefix for the bot.
#[poise::command(prefix_command, owners_only)]
pub async fn set_premium(
    ctx: Context<'_>,
    #[description = "True or false setting for premium."] set_premium: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let guild_name = get_guild_name(ctx.serenity_context(), guild_id).unwrap_or_default();

    ctx.data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            let tmp = e.premium;
            e.premium = set_premium.parse::<bool>().unwrap_or(tmp);
        });

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!(
            "Current additional prefixes {}",
            additional_prefixes.join(", ")
        )),
    )
    .await?;
    Ok(())
}
