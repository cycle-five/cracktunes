use crate::{
    commands::cmd_check_music,
    db::{aux_metadata_to_db_structures, playlist::Playlist, Metadata},
    messaging::message::CrackedMessage,
    sources::sleevenote,
    utils::send_reply,
    Context, CrackedError, Error,
};
use crack_types::NewAuxMetadata;
use songbird::input::AuxMetadata;

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

/// Get the tracks of a Spotify playlist, album, or single track, as metadata.
///
/// Any Spotify entity is accepted, not only a playlist: loading one album into
/// a named playlist is a reasonable thing to want, and refusing it bought
/// nothing. Resolution goes through sleevenote like every other Spotify path
/// in the bot -- the rspotify client this used to call has been unable to
/// authenticate since Spotify stopped issuing Web API credentials.
pub async fn get_spotify_playlist(url: &str) -> Result<Vec<NewAuxMetadata>, CrackedError> {
    let resolution = sleevenote::resolve_spotify(url).await?;
    tracing::info!(
        "spotify {}: {} -> {} track(s), {} unresolved",
        resolution.media_type.noun(),
        resolution.name,
        resolution.len(),
        resolution.unresolved,
    );
    Ok(resolution.metadata())
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

    let metadata = get_spotify_playlist(&spotifyurl).await?;

    let db_pool = get_db_or_err!(ctx);

    let mut metadata_vec: Vec<AuxMetadata> = Vec::new();
    let playls = Playlist::create(db_pool, &name.clone(), ctx.author().id.get() as i64).await?;
    let guild_id_i64 = guild_id.get() as i64;
    let channel_id_i64 = channel_id.get() as i64;
    for NewAuxMetadata(m) in metadata {
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
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    prefix_command,
    slash_command
)]
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
