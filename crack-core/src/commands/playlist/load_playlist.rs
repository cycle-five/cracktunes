use crate::{
    commands::{get_query_type_from_url, QueryType},
    db::{playlist::Playlist, Metadata},
    Context, CrackedError, Error,
};
use songbird::input::Input as SongbirdInput;

use url::Url;

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "loadspotify")]
pub async fn loadspotify(
    ctx: Context<'_>,
    #[description = "Name of the playlist to create and load into."] name: String,
    #[rest]
    #[description = "Spotify.com url to the *public* playlist."]
    spotifyurl: String,
) -> Result<(), Error> {
    // verify url format

    use crate::{
        commands::{get_track_source_and_metadata, CrackedMessage, MyAuxMetadata},
        db::aux_metadata_to_db_structures,
        utils::send_embed_response_str,
    };

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let channel_id = ctx.channel_id();

    let url_clean = Url::parse(&spotifyurl.clone())?;

    let query_type: QueryType = match get_query_type_from_url(ctx, url_clean.as_ref(), None).await?
    {
        Some(QueryType::KeywordList(v)) => QueryType::KeywordList(v),
        _ => return Err(CrackedError::Other("Bad Query Type").into()),
    };

    // let (aux_metadata, playlist_name): (Vec<AuxMetadata>, String) =
    //     get_playlist_(ctx, playlist).await?;
    let (_source, metadata): (SongbirdInput, Vec<MyAuxMetadata>) =
        get_track_source_and_metadata(ctx.http(), query_type.clone()).await;
    // let embed = build_tracks_embed_metadata(playlist_name, &aux_metadata, 0).await;
    let db_pool = ctx
        .data()
        .database_pool
        .as_ref()
        .ok_or(CrackedError::NoDatabasePool)?;

    let playls = Playlist::create(
        db_pool,
        &name.clone(),
        ctx.author().id.get() as i64,
        // guild_id.get() as i64,
        // channel_id.get() as i64,
    )
    .await?;
    let guild_id_i64 = guild_id.get() as i64;
    let channel_id_i64 = channel_id.get() as i64;
    for MyAuxMetadata::Data(m) in metadata {
        let res = aux_metadata_to_db_structures(&m, guild_id_i64, channel_id_i64);
        match res {
            Ok((in_metadata, _track)) => {
                let metadata = Metadata::get_or_create(db_pool, &in_metadata).await?;

                let _res = Playlist::add_track(
                    db_pool,
                    playls.id,
                    metadata.id,
                    guild_id_i64,
                    channel_id_i64,
                )
                .await?;
            }
            Err(e) => {
                tracing::error!("Error converting metadata to aux metadata: {}", e);
            }
        }
    }

    // Send the embed
    send_embed_response_str(ctx, CrackedMessage::PlaylistCreated(name).to_string()).await?;

    Ok(())
}
