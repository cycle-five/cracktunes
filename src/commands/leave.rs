use crate::{
    errors::CrackedError, is_prefix, messaging::message::CrackedMessage, utils::count_command,
    utils::create_response_poise, Context, Error,
};

/// Leave the current voice channel.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn leave(ctx: Context<'_>) -> Result<(), Error> {
    count_command("leave", is_prefix(ctx));
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or(CrackedError::NotConnected)?;
    let _ = manager.remove(guild_id).await?;

    create_response_poise(ctx, CrackedMessage::Leaving).await
}
