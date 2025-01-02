use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_reply;
use crate::{Context, Error};
use crack_types::CrackedError;

/// Get the all log channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    aliases("get_all_log_channel")
)]
pub async fn all_log_channel(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    {
        let all_log_channel = {
            let data = ctx.data();
            let mut guild_settings_map = data.guild_settings_map.write().await;
            let settings = guild_settings_map
                .entry(guild_id)
                .or_insert(GuildSettings::new(
                    guild_id,
                    Some(ctx.prefix()),
                    get_guild_name(ctx.serenity_context(), guild_id).await,
                ));
            settings.get_all_log_channel()
        };

        send_reply(
            &ctx,
            CrackedMessage::Other(format!(
                "All Log Channel: {:?}",
                all_log_channel.unwrap_or_default()
            )),
            true,
        )
        .await?;
    }

    Ok(())
}

/// Get the join/leave log channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn join_leave_log_channel(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(crack_types::CrackedError::NoGuildId)?;
    let name = get_guild_name(ctx.serenity_context(), guild_id).await;
    {
        let join_leave_log_channel = {
            let data = ctx.data();
            let mut guild_settings_map = data.guild_settings_map.write().await;
            let settings = guild_settings_map
                .entry(guild_id)
                .or_insert(GuildSettings::new(guild_id, Some(ctx.prefix()), name));
            settings.get_all_log_channel()
        };

        send_reply(
            &ctx,
            CrackedMessage::Other(format!(
                "Join/Leave Log Channel: {:?}",
                join_leave_log_channel.unwrap_or_default()
            )),
            true,
        )
        .await?;
    }

    Ok(())
}
