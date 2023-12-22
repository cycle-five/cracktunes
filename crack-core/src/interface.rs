use crate::{
    messaging::message::CrackedMessage,
    utils::{get_footer_info, get_human_readable_timestamp, get_track_metadata},
};
use poise::CreateReply;
use serenity::{
    all::{ButtonStyle, CreateEmbed},
    builder::{CreateActionRow, CreateButton, CreateEmbedAuthor, CreateEmbedFooter},
};
use songbird::tracks::TrackHandle;

pub async fn create_now_playing_embed(track: &TrackHandle) -> CreateEmbed {
    let metadata = get_track_metadata(track).await;
    let title = metadata.title.clone().unwrap_or_default();
    let source_url = metadata.source_url.clone().unwrap_or_default();

    let position = get_human_readable_timestamp(Some(track.get_info().await.unwrap().position));
    let duration = get_human_readable_timestamp(metadata.duration);

    let progress_field = ("Progress", format!(">>> {} / {}", position, duration), true);

    let channel_field: (&'static str, String, bool) = match metadata.channel.clone() {
        Some(channel) => ("Channel", format!(">>> {}", channel), true),
        None => ("Channel", ">>> N/A".to_string(), true),
    };

    let thumbnail = metadata.thumbnail.clone().unwrap_or_default();

    let (footer_text, footer_icon_url) = get_footer_info(&source_url);

    CreateEmbed::new()
        .author(CreateEmbedAuthor::new(CrackedMessage::NowPlaying))
        .title(title.clone())
        .url(source_url)
        .field(progress_field.0, progress_field.1, progress_field.2)
        .field(channel_field.0, channel_field.1, channel_field.2)
        // .thumbnail(url::Url::parse(&thumbnail).unwrap())
        .thumbnail(
            url::Url::parse(&thumbnail)
                .map(|x| x.to_string())
                .map_err(|e| {
                    tracing::error!("error parsing url: {:?}", e);
                    "".to_string()
                })
                .unwrap_or_default(),
        )
        .footer(CreateEmbedFooter::new(footer_text).icon_url(footer_icon_url))
}

pub async fn create_lyrics_embed_old(track: String, artists: String, lyric: String) -> CreateEmbed {
    CreateEmbed::default()
        .author(CreateEmbedAuthor::new(artists))
        .title(track)
        .description(lyric)
}

pub async fn create_search_results_reply(results: Vec<CreateEmbed>) -> CreateReply {
    let mut reply = CreateReply::default()
        .reply(true)
        .content("Search results:");
    for result in results {
        reply.embeds.push(result);
    }

    reply.clone()
}

use crate::errors::CrackedError;
use crate::Context as CrackContext;
/// Created a paging embed for the lyrics of a song.
#[cfg(not(tarpaulin_include))]
pub async fn create_lyrics_embed(
    ctx: CrackContext<'_>,
    track: String,
    artists: String,
    lyric: String,
) -> Result<(), CrackedError> {
    use crate::{guild::settings::DEFAULT_LYRICS_PAGE_SIZE, utils::create_paged_embed};

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
pub fn build_single_nav_btn(label: &str, is_disabled: bool) -> CreateButton {
    CreateButton::new(label.to_string().to_ascii_lowercase())
        .label(label)
        .style(ButtonStyle::Primary)
        .disabled(is_disabled)
        .to_owned()
}

/// Builds the four navigation buttons for the queue.
pub fn build_nav_btns(page: usize, num_pages: usize) -> Vec<CreateActionRow> {
    let (cant_left, cant_right) = (page < 1, page >= num_pages - 1);
    vec![CreateActionRow::Buttons(vec![
        build_single_nav_btn("<<", cant_left),
        build_single_nav_btn("<", cant_left),
        build_single_nav_btn(">", cant_right),
        build_single_nav_btn(">>", cant_right),
    ])]
}
