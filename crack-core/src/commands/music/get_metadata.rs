use crate::commands::cmd_check_music;
use crate::http_utils;
use crate::messaging::interface as msg_int;
use crate::music::query::query_type_from_url;
use crate::CrackedMessage;
use crate::{
    errors::{verify, CrackedError},
    Context, Error,
};
use rusty_ytdl::{search::YouTube, RequestOptions};

#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn get_metadata(ctx: Context<'_>, query_or_url: String) -> Result<(), Error> {
    let search_msg = msg_int::send_search_message(&ctx).await?;
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
        .map(|x| x.0.title.clone().unwrap_or("NOTITLE".to_string()))
        .collect::<Vec<_>>()
        .join("\n");

    let crack_msg = CrackedMessage::Other(str);
    match crate::utils::edit_embed_response2(ctx, crack_msg.into(), search_msg.clone()).await {
        Ok(_) => {},
        Err(e) => {
            tracing::error!("Error editing embed: {:?}", e);
        },
    };

    Ok(())
}
