use crate::commands::{cmd_check_music, help};
use crate::messaging::interface as msg_int;
use crate::music::query::query_type_from_url;
use crate::music::query::QueryType;
use crate::music::queue::{get_mode, get_msg, queue_track_back};
use crate::poise_ext::MessageInterfaceCtxExt;
use crate::poise_ext::PoiseContextExt;
use crate::sources::rusty_ytdl::search_result_to_aux_metadata;
use crate::utils::edit_embed_response2;
use crate::CrackedResult;
use crate::{commands::get_call_or_join_author, http_utils::SendMessageParams};
use crate::{db::metadata, http_utils};
use crate::{
    errors::{verify, CrackedError},
    guild::settings::GuildSettings,
    handlers::track_end::update_queue_messages,
    messaging::interface::create_now_playing_embed,
    messaging::{
        message::CrackedMessage,
        messages::{
            PLAY_QUEUE, PLAY_TOP, QUEUE_NO_SRC, QUEUE_NO_TITLE, TRACK_DURATION, TRACK_TIME_TO_PLAY,
        },
    },
    sources::spotify::SpotifyTrack,
    sources::youtube::build_query_aux_metadata,
    utils::{get_human_readable_timestamp, get_track_handle_metadata},
    Context, Error,
};
use ::serenity::all::CommandInteraction;
use ::serenity::{
    all::{Message, UserId},
    builder::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, EditMessage},
};
use poise::serenity_prelude as serenity;
use songbird::{
    input::{AuxMetadata, YoutubeDl},
    tracks::TrackHandle,
    Call,
};
use std::{cmp::Ordering, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use typemap_rev::TypeMapKey;

#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    guild_only,
    check = "cmd_check_music",
    category = "Music"
)]
pub async fn get_metadata(ctx: Context<'_>, query_or_url: String) -> Result<(), Error> {
    let mut search_msg = msg_int::send_search_message(&ctx).await?;
    tracing::debug!("search response msg: {:?}", search_msg);

    let query_type = query_type_from_url(ctx, &query_or_url, None).await?;

    let query_type = verify(
        query_type,
        CrackedError::Other("Something went wrong while parsing your query!"),
    )?;

    tracing::warn!("query_type: {:?}", query_type);

    let opts = RequestOptions {
        client: Some(http_utils::get_client().clone()),
        ..Default::default()
    };
    let reqclient = http_utils::get_client().clone();
    let ytclient = YouTube::new_with_options(&opts).unwrap();
    let metadata = query_type.get_track_metadata(ytclient, reqclient).await?;
    tracing::warn!("metadata: {:?}", metadata);
    let str = metadata
        .iter()
        .map(|x| x.title.clone())
        .collect::<Vec<_>>()
        .join("\n");

    let message = CrackedMessage::Other(str);

    ctx.send_reply(message, true).await?;

    Ok(())
}
