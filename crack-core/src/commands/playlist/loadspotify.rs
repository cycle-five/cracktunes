use crate::{
    commands::MyAuxMetadata,
    db::{aux_metadata_to_db_structures, playlist::Playlist, Metadata},
    errors::verify,
    http_utils,
    messaging::message::CrackedMessage,
    sources::spotify::{Spotify, SpotifyTrack, SPOTIFY},
    utils::send_reply,
    Context, CrackedError, Error,
};
use songbird::input::AuxMetadata;
use url::Url;

/// Get the database pool or return an error.
#[macro_export]
macro_rules! get_db_or_err {
    ($ctx:expr) => {
        $ctx.data()
            .database_pool
            .as_ref()
            .ok_or(CrackedError::NoDatabasePool)?
    };
}

/// Get a Spotify playlist.
pub async fn get_spotify_playlist(url: &str) -> Result<Vec<SpotifyTrack>, CrackedError> {
    let url_clean = Url::parse(url)?;

    let final_url = http_utils::resolve_final_url(url_clean.as_ref()).await?;
    tracing::warn!("spotify: {} -> {}", url_clean, final_url);
    let spotify = SPOTIFY.lock().await;
    let spotify = verify(spotify.as_ref(), CrackedError::SpotifyAuth)?;
    tracing::warn!("Getting playlist tracks...");
    Spotify::extract_tracks(spotify, &final_url).await
}

/// Load a Spotify playlist into the bot
#[cfg(not(tarpaulin_include))]
pub async fn loadspotify_(
    ctx: Context<'_>,
    name: String,
    spotifyurl: String,
) -> Result<Vec<AuxMetadata>, Error> {
    use crate::db::MetadataAnd;

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let channel_id = ctx.channel_id();

    let playlist_tracks = get_spotify_playlist(&spotifyurl).await?;

    tracing::warn!("Got playlist tracks: {:?}", playlist_tracks);

    let metadata = playlist_tracks
        .iter()
        .map(Into::<MyAuxMetadata>::into)
        .collect::<Vec<_>>();

    let db_pool = get_db_or_err!(ctx);

    let mut metadata_vec: Vec<AuxMetadata> = Vec::new();
    let playls = Playlist::create(db_pool, &name.clone(), ctx.author().id.get() as i64).await?;
    let guild_id_i64 = guild_id.get() as i64;
    let channel_id_i64 = channel_id.get() as i64;
    for MyAuxMetadata::Data(m) in metadata {
        metadata_vec.push(m.clone());
        let res = aux_metadata_to_db_structures(&m, guild_id_i64, channel_id_i64);
        match res {
            Ok(MetadataAnd::Track(in_metadata, _)) => {
                let metadata = Metadata::get_or_create(db_pool, &in_metadata).await?;

                let _res = Playlist::add_track(
                    db_pool,
                    playls.id,
                    metadata.id,
                    guild_id_i64,
                    channel_id_i64,
                )
                .await?;
            },
            Err(e) => {
                tracing::error!("Error converting metadata to aux metadata: {}", e);
            },
        }
    }
    Ok(metadata_vec)
}

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command)]
pub async fn loadspotify(
    ctx: Context<'_>,
    #[description = "Spotify.com url to the *public* playlist."] spotifyurl: String,
    #[rest]
    #[description = "Name of the playlist to create and load into."]
    name: String,
) -> Result<(), Error> {
    tracing::warn!("Loading Spotify playlist: {}", spotifyurl);
    tracing::warn!("Playlist name: {}", name);

    let metadata_vec = loadspotify_(ctx, name.to_string(), spotifyurl).await?;

    let len = metadata_vec.len();

    // Send the embed
    send_reply(&ctx, CrackedMessage::PlaylistCreated(name, len), false).await?;

    Ok(())
}
