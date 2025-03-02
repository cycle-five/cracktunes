use crate::commands::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_reply;
use crate::{Context, Error};

/// Get the current bot settings for this guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    aliases("get_all_settings")
)]
pub async fn all(
    ctx: Context<'_>,
    #[flag]
    #[description = "Shows the help menu for this command."]
    help: bool,
) -> Result<(), Error> {
    if help {
        crate::commands::help::wrapper(ctx).await?;
    }
    get_settings(ctx).await
}

/// Get the current bot settings for this guild.
pub async fn get_settings(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let settings_ro = {
        let data = ctx.data();
        let mut guild_settings_map = data.guild_settings_map.write().await;
        let settings = guild_settings_map
            .entry(guild_id)
            .or_insert(GuildSettings::new(
                guild_id,
                Some(ctx.prefix()),
                get_guild_name(ctx.serenity_context(), guild_id).await,
            ));
        settings.clone()
    };

    send_reply(
        &ctx,
        CrackedMessage::Other(format!("Settings: {settings_ro:?}")),
        true,
    )
    .await?;

    Ok(())
}
