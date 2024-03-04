use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::get_guild_name;
use crate::utils::send_response_poise;
use crate::{Context, Error};

/// Get the all log channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    ephemeral,
    default_member_permissions = "ADMINISTRATOR",
    aliases("get_all_log_channel")
)]
pub async fn all_log_channel(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    {
        let all_log_channel = {
            let mut guild_settings_map = ctx.data().guild_settings_map.write().unwrap();
            let settings = guild_settings_map
                .entry(guild_id)
                .or_insert(GuildSettings::new(
                    guild_id,
                    Some(ctx.prefix()),
                    get_guild_name(ctx.serenity_context(), guild_id),
                ));
            settings.get_all_log_channel()
        };

        send_response_poise(
            ctx,
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
    slash_command,
    prefix_command,
    default_member_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn join_leave_log_channel(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or(crate::errors::CrackedError::NoGuildId)?;
    {
        let join_leave_log_channel = {
            let mut guild_settings_map = ctx.data().guild_settings_map.write().unwrap();
            let settings = guild_settings_map
                .entry(guild_id)
                .or_insert(GuildSettings::new(
                    guild_id,
                    Some(ctx.prefix()),
                    get_guild_name(ctx.serenity_context(), guild_id),
                ));
            settings.get_all_log_channel()
        };

        send_response_poise(
            ctx,
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
