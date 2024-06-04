use crate::utils::send_now_playing;
use crate::{errors::CrackedError, Context, Error};

/// interface::create_now_playing_embed,
/// Send the current tack to your DMs.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, aliases("save"), guild_only)]
pub async fn grab(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NotConnected)?;
    let call = manager.get(guild_id).ok_or(CrackedError::NotConnected)?;
    let channel = ctx
        .author()
        .create_dm_channel(&ctx.serenity_context().http)
        .await?;

    let msg = send_now_playing(
        channel.id,
        ctx.serenity_context().http.clone(),
        call.clone(),
        None,
        None,
    )
    .await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    let reply_handle = ctx.say("Sent you a DM with the current track").await?;

    let msg = reply_handle.into_message().await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}
