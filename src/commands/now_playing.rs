use crate::{
    errors::CrackedError,
    utils::{create_embed_response_poise, create_now_playing_embed},
    Context, Error,
};

/// Get the currently playing track.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn now_playing(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let track = handler
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;

    let embed = create_now_playing_embed(&track).await;
    create_embed_response_poise(ctx, embed).await
}
