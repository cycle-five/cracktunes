// Poise keeps command attributes on its inner function, not the generated factory.
#![allow(
    unreachable_code,
    reason = "Poise's generated rest-string error branches trigger this lint on Rust 1.100"
)]

use std::sync::LazyLock;

use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    messaging::interface::create_lyrics_embed,
    poise_ext::ContextExt,
    Context, Error,
};

/// One LRCLIB client for the whole bot. It holds the cool-down from LRCLIB's
/// `Retry-After`, so after a 429 every guild waits it out instead of each
/// command hitting the server again. It builds its own reqwest 0.13 client
/// (HTTPS only, timeouts, same-origin redirects); the bot's shared one is 0.12.
static LYRICS: LazyLock<lyric_finder::Client> = LazyLock::new(lyric_finder::Client::new);

/// Search for song lyrics.
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command,
    guild_only
)]
pub async fn lyrics(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
    #[rest]
    #[description = "The search query."]
    query: Option<String>,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    lyrics_internal(ctx, query).await
}

/// Internal function for searching for song lyrics.
pub async fn lyrics_internal(ctx: Context<'_>, query: Option<String>) -> Result<(), Error> {
    let query = query_or_title(ctx, query).await?;
    tracing::warn!("searching for lyrics for {}", query);

    let res = LYRICS.get_lyric(&query).await?;
    create_lyrics_embed(ctx, res).await.map_err(Into::into)
}

/// Get the current track name as either the query or the title of the current track.
pub async fn query_or_title(ctx: Context<'_>, query: Option<String>) -> Result<String, Error> {
    match query {
        Some(query) => Ok(query),
        None => {
            let call = ctx.get_call().await?;
            let handler = call.lock().await;
            let track_handle = handler
                .queue()
                .current()
                .ok_or(CrackedError::NothingPlaying)?;

            let metadata = crate::utils::get_track_handle_metadata(&track_handle)
                .await
                .unwrap_or_default();
            tracing::info!("metadata: {:?}", metadata);
            metadata
                .track
                .clone()
                .or(metadata.title.clone())
                .ok_or(CrackedError::NoTrackName.into())
        },
    }
}
