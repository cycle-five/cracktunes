use crate::{errors::CrackedError, interface::create_search_results_reply, Context, Error};
use poise::ReplyHandle;
use reqwest::Client;
use serenity::builder::CreateEmbed;
use songbird::input::YoutubeDl;

/// Search for a song and play it.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn do_yt_search(
    ctx: Context<'_>,
    #[rest]
    #[description = "Search query."]
    search_query: String,
) -> Result<(), Error> {
    do_yt_search_internal(ctx, search_query)
        .await
        .map(|_| ())
        .map_err(Into::into)
}

/// Perform a youtube search and send a list of results to discord
#[cfg(not(tarpaulin_include))]
async fn do_yt_search_internal(
    ctx: Context<'_>,
    search_query: String,
) -> Result<ReplyHandle, CrackedError> {
    let mut ytdl = YoutubeDl::new(Client::new(), search_query);
    let results = ytdl.search(None).await?;

    let embeds = results
        .into_iter()
        .enumerate()
        .filter_map(|(i, result)| {
            if i != 0 {
                Some(
                    CreateEmbed::default()
                        .title(format!("({})[{}]", i, result.title.unwrap_or_default()))
                        .url(result.source_url.unwrap_or_default()),
                )
            } else {
                None
            }
        })
        .collect::<Vec<CreateEmbed>>();
    for (i, embed) in embeds.iter().enumerate() {
        tracing::warn!("i: {}, embed: {:?}", i, embed);
    }
    ctx.send(create_search_results_reply(embeds).await)
        .await
        .map_err(Into::into)
}
