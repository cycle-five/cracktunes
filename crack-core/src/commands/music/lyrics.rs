use crate::{
    commands::{cmd_check_music, help},
    errors::CrackedError,
    http_utils,
    messaging::interface::create_lyrics_embed,
    poise_ext::ContextExt,
    Context, Error,
};
use crack_types::NewAuxMetadata;

/// Search for song lyrics.
#[cfg(not(tarpaulin_include))]
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

#[cfg(not(tarpaulin_include))]
/// Internal function for searching for song lyrics.
pub async fn lyrics_internal(ctx: Context<'_>, query: Option<String>) -> Result<(), Error> {
    let query = query_or_title(ctx, query).await?;
    tracing::warn!("searching for lyrics for {}", query);

    let client = lyric_finder::Client::from_http_client(http_utils::get_client());

    let res = client.get_lyric(&query).await?;
    create_lyrics_embed(ctx, res).await.map_err(Into::into)
}

/// Get the current track name as either the query or the title of the current track.
#[cfg(not(tarpaulin_include))]
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

            let lock = track_handle.typemap().read().await;
            let NewAuxMetadata(data) = lock.get::<NewAuxMetadata>().unwrap();
            tracing::info!("data: {:?}", data);
            data.track
                .clone()
                .or(data.title.clone())
                .ok_or(CrackedError::NoTrackName.into())
        },
    }
}
