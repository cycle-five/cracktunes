use crate::connection::get_voice_channel_for_user;
use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{Context, Error};
use std::collections::HashMap;

use poise::serenity_prelude as serenity;
use serenity::{CacheHttp, EditMember, GuildId, Mentionable};

/// Mute a user.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn mute(
    ctx: Context<'_>,
    #[description = "User to mute"] user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let crack_msg = mute_internal(&ctx, user, guild_id, true).await?;
    send_reply(&ctx, crack_msg, true)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

/// Mutes all other users in the voice channel.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn mute_others(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // Get the voice channel of the user
    let author = ctx.author();

    let (voice_channel, voice_states) = {
        let guild = guild_id
            .to_guild_cached(&ctx)
            .ok_or(CrackedError::NoGuildCached)?;
        let voice_states = guild.voice_states.clone();
        (
            get_voice_channel_for_user(&guild, author.id.as_ref())?,
            voice_states,
        )
    };
    mute_others_internal(&ctx, guild_id, voice_states, author.id, voice_channel, true)
        .await
        .map(|_| ())
        .map_err(Into::into)
    // match mute_others_internal(&ctx, guild_id, &guild, author.id, voice_channel, true).await {
    //     Ok(crack_msg) => send_reply(&ctx, crack_msg, true)
    //         .await
    //         .map(|_| ())
    //         .map_err(Into::into),
    //     Err(e) => {
    //         tracing::error!("Failed to mute others: {}", e);
    //         send_reply(
    //             &ctx,
    //             CrackedMessage::Other(format!("Failed to mute others: {}", e)),
    //             true,
    //         )
    //         .await
    //         .map(|_| ())
    //         .map_err(Into::into)
    //     },
    // }
}

/// All over users in a voice channel.
pub async fn mute_others_internal(
    ctx: impl AsRef<serenity::Cache> + CacheHttp,
    guild_id: GuildId,
    voice_states: HashMap<serenity::all::UserId, serenity::all::VoiceState>,
    user_id: serenity::model::id::UserId,
    voice_channel: serenity::model::id::ChannelId,
    mute: bool,
) -> Result<CrackedMessage, Error> {
    // let guild = guild_id
    //     .to_guild_cached(&ctx)
    //     .ok_or(CrackedError::NoGuildCached)?;
    // // Get all members in the voice channel
    // let members = guild
    // .voice_states
    let members = voice_states
        .values()
        .filter(|vs| vs.channel_id == Some(voice_channel))
        .map(|vs| vs.user_id)
        .filter(|id| id != &user_id)
        .collect::<Vec<_>>();

    for member_id in members {
        let user = member_id.to_user(&ctx).await?;
        if let Err(e) = mute_internal(&ctx, user, guild_id, mute).await {
            tracing::warn!("Failed to mute user: {}", e);
        }
    }
    Ok(CrackedMessage::Other("Done.".to_string()))
}

/// Mute a user, internal function.
pub async fn mute_internal(
    cache_http: &impl CacheHttp,
    user: serenity::User,
    guild_id: GuildId,
    mute: bool,
) -> Result<CrackedMessage, Error> {
    let mention = user.mention();
    let id = user.id;
    if let Err(e) = guild_id
        .edit_member(cache_http, user.clone().id, EditMember::new().mute(mute))
        .await
    {
        Ok(CrackedMessage::Other(format!("Failed to mute user: {}", e)))
    } else {
        // Send success message
        Ok(CrackedMessage::UserMuted { mention, id })
    }
}
