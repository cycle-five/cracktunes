use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_response_poise;
use crate::{Context, Error};

#[cfg(not(tarpaulin_include))]
#[poise::command(
    prefix_command,
    owners_only,
    ephemeral,
    aliases("get_welcome_settings")
)]
pub async fn welcome_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let welcome_settings = {
        let mut guild_settings_map = ctx.data().guild_settings_map.write().unwrap();
        let settings = guild_settings_map
            .entry(guild_id)
            .or_insert(GuildSettings::new(
                guild_id,
                Some(ctx.prefix()),
                get_guild_name(ctx.serenity_context(), guild_id),
            ));
        settings.welcome_settings.clone()
    };

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!(
            "Welcome settings: {:?}",
            welcome_settings.unwrap_or_default()
        )),
        true,
    )
    .await?;

    Ok(())
}
