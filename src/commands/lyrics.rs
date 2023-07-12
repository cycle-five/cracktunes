use crate::{
    errors::CrackedError,
    utils::{count_command, create_embed_response_poise, create_lyrics_embed},
    Context, Error,
};

/// Get the currently playing track.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn lyrics(ctx: Context<'_>, query: Option<String>) -> Result<(), Error> {
    count_command("lyrics");
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let manager = songbird::get(ctx.serenity_context()).await.unwrap();
    let call = manager.get(guild_id).unwrap();

    let handler = call.lock().await;
    let track_handle = handler
        .queue()
        .current()
        .ok_or(CrackedError::NothingPlaying)?;

    let search_title = track_handle.metadata().title.clone().unwrap_or_default();
    let search_artist = track_handle.metadata().artist.clone().unwrap_or_default();
    let query = query.unwrap_or_else(|| format!("{} {}", search_title, search_artist));

    let client = lyric_finder::Client::new();
    let result = client.get_lyric(&query).await?;
    let (track, artists, lyric) = match result {
        lyric_finder::LyricResult::Some {
            track,
            artists,
            lyric,
        } => {
            tracing::warn!("{} by {}'s lyric:\n{}", track, artists, lyric);
            (track, artists, lyric)
        }
        lyric_finder::LyricResult::None => {
            tracing::error!("lyric not found! query: {}", query);
            (
                "Unknown".to_string(),
                "Unknown".to_string(),
                "Lyric not found!".to_string(),
            )
        }
    };

    let embed = create_lyrics_embed(track_handle, track, artists, lyric).await;
    create_embed_response_poise(ctx, embed).await
}
