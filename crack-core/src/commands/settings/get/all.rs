use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_response_poise;
use crate::{Context, Error};

/// Get the current bot settings for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral,
    aliases("get_all_settings")
)]
pub async fn all(ctx: Context<'_>) -> Result<(), Error> {
    get_settings(ctx).await
}

/// Get the current bot settings for this guild.
pub async fn get_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let settings_ro = {
        let mut guild_settings_map = ctx.data().guild_settings_map.write().await;
        let settings = guild_settings_map
            .entry(guild_id)
            .or_insert(GuildSettings::new(
                guild_id,
                Some(ctx.prefix()),
                get_guild_name(ctx.serenity_context(), guild_id),
            ));
        settings.clone()
    };

    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Settings: {:?}", settings_ro)),
        true,
    )
    .await?;

    Ok(())
}
