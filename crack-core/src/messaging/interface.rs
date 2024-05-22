use super::messages::REQUESTED_BY;
use crate::errors::CrackedError;
use crate::messaging::messages::{
    QUEUE_NOTHING_IS_PLAYING, QUEUE_NOW_PLAYING, QUEUE_NO_SONGS, QUEUE_NO_SRC, QUEUE_NO_TITLE,
    QUEUE_PAGE, QUEUE_PAGE_OF, QUEUE_UP_NEXT,
};
use crate::utils::EMBED_PAGE_SIZE;
use crate::utils::{calculate_num_pages, send_embed_response_poise};
use crate::Context as CrackContext;
use crate::{guild::settings::DEFAULT_LYRICS_PAGE_SIZE, utils::create_paged_embed};
use crate::{
    messaging::message::CrackedMessage,
    utils::{
        get_footer_info, get_human_readable_timestamp, get_requesting_user, get_track_metadata,
    },
};
/// Contains functions for creating embeds and other messages which are used
/// to communicate with the user.
use lyric_finder::LyricResult;
use poise::CreateReply;
use serenity::all::UserId;
use serenity::{
    all::Mentionable,
    all::{ButtonStyle, CreateEmbed},
    builder::{CreateActionRow, CreateButton, CreateEmbedAuthor, CreateEmbedFooter},
};
use songbird::tracks::TrackHandle;
use std::fmt::Write;

/// Converts a user id to a string, with special handling for autoplay.
pub fn requesting_user_to_string(user_id: UserId) -> String {
    match user_id.get() {
        1 => "(auto)".to_string(),
        _ => user_id.mention().to_string(),
    }
}

/// Creates a page of the queue.
#[cfg(not(tarpaulin_include))]
async fn create_queue_page(tracks: &[TrackHandle], page: usize) -> String {
    let start_idx = EMBED_PAGE_SIZE * page;
    let queue: Vec<&TrackHandle> = tracks
        .iter()
        .skip(start_idx + 1)
        .take(EMBED_PAGE_SIZE)
        .collect();

    if queue.is_empty() {
        return String::from(QUEUE_NO_SONGS);
    }

    let mut description = String::new();

    for (i, &t) in queue.iter().enumerate() {
        let metadata = get_track_metadata(t).await;
        let title = metadata.title.clone().unwrap_or_default();
        let url = metadata.source_url.clone().unwrap_or_default();
        let duration = get_human_readable_timestamp(metadata.duration);
        let requesting_user = get_requesting_user(t).await.unwrap_or(UserId::new(1));

        let _ = writeln!(
            description,
            "{}. [{}]({}) • {} ({})",
            i + start_idx + 1,
            title,
            url,
            duration,
            requesting_user_to_string(requesting_user),
        );
    }

    description
}

/// Creates a queue embed.
pub async fn create_queue_embed(tracks: &[TrackHandle], page: usize) -> CreateEmbed {
    let (description, thumbnail) = if !tracks.is_empty() {
        let metadata = get_track_metadata(&tracks[0]).await;

        let url = metadata.thumbnail.clone().unwrap_or_default();
        let thumbnail = match url::Url::parse(&url) {
            Ok(url) => url.to_string(),
            Err(e) => {
                tracing::error!("error parsing url: {:?}", e);
                "".to_string()
            },
        };

        let description = format!(
            "[{}]({}) • {}",
            metadata
                .title
                .as_ref()
                .unwrap_or(&String::from(QUEUE_NO_TITLE)),
            metadata
                .source_url
                .as_ref()
                .unwrap_or(&String::from(QUEUE_NO_SRC)),
            get_human_readable_timestamp(metadata.duration)
        );
        (description, thumbnail)
    } else {
        (String::from(QUEUE_NOTHING_IS_PLAYING), "".to_string())
    };

    CreateEmbed::default()
        .thumbnail(thumbnail)
        .field(QUEUE_NOW_PLAYING, &description, false)
        .field(QUEUE_UP_NEXT, create_queue_page(tracks, page).await, false)
        .footer(CreateEmbedFooter::new(format!(
            "{} {} {} {}",
            QUEUE_PAGE,
            page + 1,
            QUEUE_PAGE_OF,
            calculate_num_pages(tracks),
        )))
}

/// Creates a now playing embed for the given track.
pub async fn create_now_playing_embed(track: &TrackHandle) -> CreateEmbed {
    let metadata = get_track_metadata(track).await;
    let title = metadata.title.clone().unwrap_or_default();
    let source_url = metadata.source_url.clone().unwrap_or_default();
    let requesting_user = get_requesting_user(track).await;

    let position = get_human_readable_timestamp(Some(track.get_info().await.unwrap().position));
    let duration = get_human_readable_timestamp(metadata.duration);

    let progress_field = ("Progress", format!(">>> {} / {}", position, duration), true);

    let channel_field: (&'static str, String, bool) = match requesting_user {
        Ok(user_id) => (
            REQUESTED_BY,
            format!(">>> {}", requesting_user_to_string(user_id)),
            true,
        ),
        Err(error) => {
            tracing::error!("error getting requesting user: {:?}", error);
            (REQUESTED_BY, ">>> N/A".to_string(), true)
        },
    };

    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();

    let (footer_text, footer_icon_url, vanity) = get_footer_info(&source_url);
    CreateEmbed::new()
        .author(CreateEmbedAuthor::new(CrackedMessage::NowPlaying))
        .title(title.clone())
        .url(source_url)
        .field(progress_field.0, progress_field.1, progress_field.2)
        .field(channel_field.0, channel_field.1, channel_field.2)
        .thumbnail(
            url::Url::parse(&thumbnail)
                .map(|x| x.to_string())
                .map_err(|e| {
                    tracing::error!("error parsing url: {:?}", e);
                    "".to_string()
                })
                .unwrap_or_default(),
        )
        .description(vanity)
        .footer(CreateEmbedFooter::new(footer_text).icon_url(footer_icon_url))
}

/// Creates a lyrics embed for the given track.
pub async fn create_lyrics_embed_old(track: String, artists: String, lyric: String) -> CreateEmbed {
    CreateEmbed::default()
        .author(CreateEmbedAuthor::new(artists))
        .title(track)
        .description(lyric)
}

/// Creates a search results reply.
pub async fn create_search_results_reply(results: Vec<CreateEmbed>) -> CreateReply {
    let mut reply = CreateReply::default()
        .reply(true)
        .content("Search results:");
    for result in results {
        reply.embeds.push(result);
    }

    reply.clone()
}

/// Creates a paging embed for the lyrics of a song.
#[cfg(not(tarpaulin_include))]
pub async fn create_lyrics_embed(
    ctx: CrackContext<'_>,
    lyric_res: LyricResult,
) -> Result<(), CrackedError> {
    let (track, artists, lyric) = match lyric_res {
        LyricResult::Some {
            track,
            artists,
            lyric,
        } => (track, artists, lyric),
        LyricResult::None => (
            "Unknown".to_string(),
            "Unknown".to_string(),
            "No lyrics found!".to_string(),
        ),
    };

    create_paged_embed(
        ctx,
        artists,
        track,
        lyric,
        DEFAULT_LYRICS_PAGE_SIZE, //ctx.data().bot_settings.lyrics_page_size,
    )
    .await
}

/// Builds a single navigation button for the queue.
pub fn create_single_nav_btn(label: &str, is_disabled: bool) -> CreateButton {
    CreateButton::new(label.to_string().to_ascii_lowercase())
        .label(label)
        .style(ButtonStyle::Primary)
        .disabled(is_disabled)
        .to_owned()
}

/// Builds the four navigation buttons for the queue.
pub fn create_nav_btns(page: usize, num_pages: usize) -> Vec<CreateActionRow> {
    let (cant_left, cant_right) = (page < 1, page >= num_pages - 1);
    vec![CreateActionRow::Buttons(vec![
        create_single_nav_btn("<<", cant_left),
        create_single_nav_btn("<", cant_left),
        create_single_nav_btn(">", cant_right),
        create_single_nav_btn(">>", cant_right),
    ])]
}

/// Sends a message to the user indicating that the search failed.
pub async fn send_search_failed(ctx: CrackContext<'_>) -> Result<(), CrackedError> {
    let guild_id = ctx.guild_id().unwrap();
    let embed = CreateEmbed::default()
        .description(format!(
            "{}",
            CrackedError::Other("Something went wrong while parsing your query!")
        ))
        .footer(CreateEmbedFooter::new("Search failed!"));
    let msg = send_embed_response_poise(ctx, embed).await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}

/// Sends a message to the user indicating that no query was provided.
pub async fn send_no_query_provided(ctx: CrackContext<'_>) -> Result<(), CrackedError> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let embed = CreateEmbed::default()
        .description(format!("{}", CrackedError::Other("No query provided!")))
        .footer(CreateEmbedFooter::new("No query provided!"));
    let msg = send_embed_response_poise(ctx, embed).await?;
    ctx.data().add_msg_to_cache(guild_id, msg);
    Ok(())
}

#[cfg(test)]
mod test {
    #[test]
    fn test_requesting_user_to_string() {
        use super::requesting_user_to_string;
        use serenity::model::id::UserId;

        assert_eq!(requesting_user_to_string(UserId::new(1)), "(auto)");
        assert_eq!(requesting_user_to_string(UserId::new(2)), "<@2>");
    }
}
