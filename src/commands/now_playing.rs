use crate::{
    errors::CrackedError,
    is_prefix,
    utils::{count_command, create_embed_response_poise, create_now_playing_embed, get_guild_id},
    Context, Error,
};

/// Get the currently playing track.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn now_playing(ctx: Context<'_>) -> Result<(), Error> {
    count_command("now_playing", is_prefix(ctx));
    let guild_id = get_guild_id(&ctx).unwrap();
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
