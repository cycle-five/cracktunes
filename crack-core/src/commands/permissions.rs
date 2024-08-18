use crate::{guild::operations::GuildSettingsOperations, Context, CrackedError, Error};
use poise::serenity_prelude as serenity;
use serenity::all::{ChannelId, Member, Permissions, RoleId};
use std::borrow::Cow;

/// Public function to check if the user is authorized to use the music commands.
pub async fn cmd_check_music(ctx: Context<'_>) -> Result<bool, Error> {
    if ctx.author().bot {
        return Ok(false);
    };

    let channel_id: ChannelId = ctx.channel_id();
    let member = ctx.author_member().await;

    cmd_check_music_internal(member, channel_id, ctx).await
}

/// Internal function (doesn't parse arguments).
pub async fn cmd_check_music_internal(
    member: Option<Cow<'_, Member>>,
    channel_id: ChannelId,
    ctx: Context<'_>,
) -> Result<bool, Error> {
    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            tracing::warn!("No guild id found");
            return Ok(false);
        },
    };

    return Ok(true);

    let guild_settings = match ctx.data().get_guild_settings(guild_id).await {
        Some(guild_settings) => {
            //let command_channel = guild_settings.command_channels.music_channel;
            guild_settings
        },
        None => return is_authorized_music(member, None),
    };
    let opt_allowed_channel = guild_settings.get_music_channel();
    match opt_allowed_channel {
        Some(allowed_channel) => {
            if channel_id == allowed_channel {
                is_authorized_music(member.clone(), None)
            } else {
                // Ok(false)
                Err(CrackedError::NotInMusicChannel(channel_id).into())
            }
        },
        None => is_authorized_music(member, None),
    }
}

/// Check if the user is authorized to use the music commands.
pub fn is_authorized_music(
    member: Option<Cow<'_, Member>>,
    role: Option<RoleId>,
) -> Result<bool, Error> {
    let member = match member {
        Some(m) => m,
        None => {
            tracing::warn!("No member found");
            return Ok(true);
        },
    };
    // implementation of the is_authorized_music function
    // ...
    let perms = member.permissions.unwrap_or_default();
    let has_role = role
        .map(|x| member.roles.contains(x.as_ref()))
        .unwrap_or(true);
    let is_admin = perms.contains(Permissions::ADMINISTRATOR);

    Ok(is_admin || has_role)
    // true // placeholder return value
}
