use songbird::input::AuxMetadata;
use songbird::input::Input as SongbirdInput;

use crate::{
    ::commands::{get_query_type_from_url, QueryType},
    db::{metadata::aux_metadata_from_db, playlist::Playlist, Metadata},
    utils::{build_tracks_embed_metadata, send_embed_response_poise},
    Context, CrackedError, Error,
};

use url::Url;

/// Get a playlist
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, rename = "loadspotify")]
pub async fn loadspotify(ctx: Context<'_>, #[rest] spotifyurl: String) -> Result<(), Error> {
    // verify url format

    use crate::{
        commands::{get_track_source_and_metadata, MyAuxMetadata},
        db::aux_metadata_to_db_structures,
    };

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let channel_id = ctx.channel_id();

    let url_clean = Url::parse(&spotifyurl.clone())?;

    let query_type: QueryType =
        match get_query_type_from_url(ctx, &url_clean.to_string(), None).await? {
            Some(qt) => match qt {
                QueryType::KeywordList(v) => QueryType::KeywordList(v),
                x => return Err(CrackedError::Other("Bad Query Type").into()),
            },
            x => return Err(CrackedError::Other("Bad Query Type").into()),
        };

    // let (aux_metadata, playlist_name): (Vec<AuxMetadata>, String) =
    //     get_playlist_(ctx, playlist).await?;
    let (source, metadata): (SongbirdInput, Vec<MyAuxMetadata>) =
        get_track_source_and_metadata(ctx.http(), query_type.clone()).await;
    // let embed = build_tracks_embed_metadata(playlist_name, &aux_metadata, 0).await;
    for m in metadata {
        match m {
            MyAuxMetadata::Metadata(m) => {
                let _ = aux_metadata_to_db_structures(m, guild_id, channel_id);
            }
            MyAuxMetadata::AuxMetadata(m) => {
                let _ = aux_metadata_to_db_structures(m, guild_id, channel_id);
            }
        }
        //let _ = aux_metadata_to_db_structures(m, guild_id, channel_id);
    }

    // Send the embed
    send_embed_response_poise(ctx, embed).await?;

    Ok(())
}

/// Get a playlist by name or id
pub async fn get_playlist_(
    ctx: Context<'_>,
    playlist: String,
) -> Result<(Vec<AuxMetadata>, String), Error> {
    let pool = ctx
        .data()
        .database_pool
        .as_ref()
        .ok_or(CrackedError::NoDatabasePool)?;

    // let metadata: Vec<Metadata> = match playlist.parse::<i32>() {
    //     // Try to parse the playlist as an ID
    //     Ok(playlist_id) => Playlist::get_track_metadata_for_playlist(pool, playlist_id).await?,
    //     Err(_) => {
    //         let user_id = ctx.author().id.get() as i64;

    //         Playlist::get_track_metadata_for_playlist_name(pool, playlist.clone(), user_id).await?
    //     }
    // };

    // Assuming you have a way to fetch the user_id of the command issuer
    let aux_metadata = metadata
        .iter()
        .flat_map(|m| match aux_metadata_from_db(m) {
            Ok(aux) => Some(aux),
            Err(e) => {
                tracing::error!("Error converting metadata to aux metadata: {}", e);
                None
            }
        })
        .collect::<Vec<_>>();
    // playlist.print_playlist(ctx).await?;
    Ok((aux_metadata, playlist))
}
