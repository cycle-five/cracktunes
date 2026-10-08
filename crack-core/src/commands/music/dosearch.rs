// Nightly reports `unreachable_code` inside `poise::command` (stable does not).
// An item-level allow does not cover a lint whose span is the attribute macro.
#![allow(unreachable_code)]

use crate::{
    commands::{cmd_check_music, sub_help as help},
    errors::CrackedError,
    messaging::courier,
    messaging::format::{clip, http_url, TrackLabel, EMBED_TITLE_MAX},
    messaging::interface::create_search_results_reply,
    Context, Error,
};
use poise::ReplyHandle;
use serenity::builder::CreateEmbed;
use songbird::input::{AuxMetadata, YoutubeDl};

/// Search for a song and play it.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    prefix_command,
    slash_command,
    guild_only,
    check = "cmd_check_music",
    aliases("ytsearch"),
    subcommands("help")
)]
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
) -> Result<ReplyHandle<'_>, CrackedError> {
    use crate::http_utils;

    // 🔒 `new_search`, never `new`: `new` hands yt-dlp the raw text as a
    // positional argument, where a leading `-` makes it an option.
    let mut ytdl = YoutubeDl::new_search(http_utils::get_client_old().clone(), search_query);
    let results = ytdl.search(None).await?;

    let embeds = results
        .into_iter()
        .enumerate()
        .skip(1)
        .map(|(i, hit)| search_result_embed(i, &hit))
        .collect::<Vec<_>>();
    for (i, embed) in embeds.iter().enumerate() {
        tracing::warn!("i: {}, embed: {:?}", i, embed);
    }
    courier::reply_rendered(ctx, create_search_results_reply(embeds), false).await
}

/// One search hit as an embed: `(i)[title]`, the title escaped because it is
/// YouTube's text, linked only when the link is http(s).
fn search_result_embed(i: usize, hit: &AuxMetadata) -> CreateEmbed<'static> {
    let label = TrackLabel::from_metadata(hit);
    let title = clip(
        &format!("({i})[{}]", label.title_text(EMBED_TITLE_MAX)),
        EMBED_TITLE_MAX,
    );
    let embed = CreateEmbed::new().title(title);
    match http_url(label.url.as_deref()) {
        Some(url) => embed.url(url.to_string()),
        None => embed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(title: Option<&str>, url: &str) -> AuxMetadata {
        AuxMetadata {
            title: title.map(str::to_owned),
            source_url: Some(url.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn a_search_hit_is_escaped_and_linked_only_to_the_web() {
        let v = serde_json::to_value(search_result_embed(
            2,
            &hit(Some("a*b @everyone"), "javascript:alert(1)"),
        ))
        .unwrap();
        assert_eq!(v["title"], "(2)[a\\*b \\@everyone]");
        assert!(v.get("url").is_none() || v["url"].is_null(), "{v}");

        let v =
            serde_json::to_value(search_result_embed(3, &hit(None, "https://youtu.be/x"))).unwrap();
        assert_eq!(v["title"], "(3)[(untitled)]");
        assert_eq!(v["url"], "https://youtu.be/x");
    }
}
