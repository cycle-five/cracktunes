use crate::connection::get_voice_channel_for_user;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::{Context, Error};
use crack_types::CrackedError;

use extract_map::ExtractMap;
use poise::serenity_prelude as serenity;
use serenity::model::id::{ChannelId, GuildId, UserId};
use serenity::model::{user::User, voice::VoiceState};
use serenity::{CacheHttp, EditMember, Mentionable};

/// Mute a user.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    guild_only
)]
pub async fn mute(
    ctx: Context<'_>,
    #[description = "User to mute"] user: User,
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
    guild_only
)]
pub async fn mute_others(
    ctx: Context<'_>,
    #[flag]
    #[description = "Unmute rather than mute."]
    unmute: bool,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // Get the voice channel of the user
    let author = ctx.author();

    let (voice_channel, voice_states) = {
        let guild = guild_id
            .to_guild_cached(ctx.cache())
            .ok_or(CrackedError::NoGuildCached)?;
        let voice_states = guild.voice_states.clone();
        (
            get_voice_channel_for_user(&guild, author.id.as_ref())?,
            voice_states,
        )
    };
    mute_others_internal(
        &ctx,
        guild_id,
        voice_states,
        author.id,
        voice_channel,
        !unmute,
    )
    .await
    .map(|_| ())
}

/// Mute all other users in a voice channel.
pub async fn mute_others_internal(
    ctx: impl AsRef<serenity::Cache> + CacheHttp,
    guild_id: GuildId,
    voice_states: ExtractMap<UserId, VoiceState>,
    user_id: UserId,
    voice_channel: ChannelId,
    mute: bool,
) -> Result<CrackedMessage, Error> {
    let members = voice_states
        .iter()
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
        .edit_member(
            cache_http.http(),
            user.clone().id,
            EditMember::new().mute(mute),
        )
        .await
    {
        Ok(CrackedMessage::Other(format!("Failed to mute user: {e}")))
    } else {
        // Send success message
        Ok(CrackedMessage::UserMuted { mention, id })
    }
}

/// Unmute a user.
/// TODO: Add a way to unmute a user by their ID.
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    guild_only
)]
#[cfg(not(tarpaulin_include))]
pub async fn unmute(
    ctx: Context<'_>,
    #[description = "User of unmute"] user: User,
) -> Result<(), Error> {
    unmute_impl(ctx, user).await
}

/// Unmute a user
/// impl for other internal use.
#[cfg(not(tarpaulin_include))]
pub async fn unmute_impl(ctx: Context<'_>, user: User) -> Result<(), Error> {
    let id = user.id;
    let mention = user.mention();
    let guild_id = ctx
        .guild_id()
        .ok_or(CrackedError::Other("Guild ID not found"))?;
    if let Err(e) = guild_id
        .edit_member(ctx.http(), user.clone().id, EditMember::new().mute(false))
        .await
    {
        // Handle error, send error message
        send_reply(
            &ctx,
            CrackedMessage::Other(format!("Failed to unmute user: {e}")),
            true,
        )
        .await
    } else {
        // Send success message
        send_reply(&ctx, CrackedMessage::UserUnmuted { id, mention }, true).await
    }?;
    Ok(())
}
