use crate::errors::CrackedError;
use crate::http_utils::SendMessageParams;
use crate::messaging::cards::NowPlayingCard;
use crate::messaging::format::{duration_text, Progress, TrackLabel, INLINE_TITLE_MAX};
use crate::messaging::messages::UNKNOWN;
use crate::messaging::messages::{
    QUEUE_NOTHING_IS_PLAYING, QUEUE_NOW_PLAYING, QUEUE_NO_SONGS, QUEUE_PAGE, QUEUE_PAGE_OF,
    QUEUE_UP_NEXT,
};
use crate::messaging::render::Rendered;
use crate::utils::EMBED_PAGE_SIZE;
use crate::utils::{calculate_num_pages, send_embed_response_poise};
use crate::CrackedResult;
use crate::{
    guild::settings::DEFAULT_LYRICS_PAGE_SIZE,
    utils::{create_paged_embed, PagedStyle},
};
use crate::{
    messaging::message::CrackedMessage,
    utils::{get_requesting_user, get_track_handle_metadata},
    Context as CrackContext, Error,
};
/// Contains functions for creating embeds and other messages which are used
/// to communicate with the user.
use lyric_finder::LyricResult;
use poise::ReplyHandle;
use serenity::all::EmbedField;
use serenity::all::GuildId;
use serenity::small_fixed_array::FixedString;
use serenity::{
    all::{ButtonStyle, CreateEmbed, CreateMessage, Message},
    all::{CacheHttp, GenericChannelId, Mentionable, UserId},
    builder::{
        CreateActionRow, CreateButton, CreateComponent, CreateEmbedAuthor, CreateEmbedFooter,
    },
};
use songbird::input::AuxMetadata;
use songbird::tracks::{PlayMode, TrackHandle};
use std::borrow::Cow;
use std::fmt::Write;
use std::time::Duration;

//###########################################################################//
// Methods to create embeds for specific messages from services or common
// commands.
//###########################################################################//
//

// ------ Logging output ------ //

/// Create and sends an log message as an embed.
/// FIXME: The avatar_url won't always be available. How do we best handle this?
pub async fn build_log_embed<'a>(
    title: &'a str,
    description: &'a str,
    avatar_url: &'a str,
) -> Result<CreateEmbed<'a>, CrackedError> {
    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let footer = CreateEmbedFooter::new(now_time_str);
    Ok(CreateEmbed::default()
        .title(title)
        .description(description)
        .thumbnail(avatar_url, None)
        .footer(footer))
}

/// Build a log embed with(out?) a thumbnail.
pub async fn build_log_embed_thumb<'a>(
    guild_name: &'a str,
    title: &'a str,
    id: &'a str,
    description: &'a str,
    avatar_url: &'a str,
) -> CrackedResult<CreateEmbed<'a>> {
    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let footer_str = format!("{} | {} | {}", guild_name, id, now_time_str);
    let footer = CreateEmbedFooter::new(footer_str);
    let author = CreateEmbedAuthor::new(title).icon_url(avatar_url);
    Ok(CreateEmbed::default()
        .author(author)
        // .title(title)
        .description(description)
        // .thumbnail(avatar_url)
        .footer(footer))
}

/// Send a log message as a embed with a thumbnail.
#[cfg(not(tarpaulin_include))]
#[expect(
    clippy::disallowed_methods,
    reason = "messaging is where sends are made"
)]
pub async fn send_log_embed_thumb(
    guild_name: &str,
    channel: &GenericChannelId,
    cache_http: &impl CacheHttp,
    id: &str,
    title: &str,
    description: &str,
    avatar_url: &str,
) -> Result<Message, Error> {
    let embed = build_log_embed_thumb(guild_name, title, id, description, avatar_url).await?;

    channel
        .send_message(cache_http.http(), CreateMessage::new().embed(embed))
        .await
        .map_err(Into::into)
}

// ------ Queue Display / Interaction ------ //

/// Converts a user id to a string, with special handling for autoplay.
pub fn requesting_user_to_string(user_id: UserId) -> String {
    match user_id.get() {
        1 => "(auto)".to_string(),
        _ => user_id.mention().to_string(),
    }
}

/// Creates a page of the queue's "Up next": the tracks after the one playing.
#[cfg(not(tarpaulin_include))]
async fn create_queue_page(tracks: &[TrackHandle], page: usize) -> String {
    let start_idx = EMBED_PAGE_SIZE * page;
    // `+ 1`: the first track is the one playing, shown above as "Now playing",
    // and `calculate_num_pages` counts only what comes after it. Without it a
    // one-song queue listed that song twice.
    let queue = tracks.iter().skip(start_idx + 1).take(EMBED_PAGE_SIZE);

    let mut description = String::new();

    for (i, t) in queue.enumerate() {
        // A track can have no metadata (a pick nothing resolved a title for).
        // It gets a blank line here, not a panic that kills `/queue`.
        let metadata = get_track_handle_metadata(t).await.unwrap_or_default();
        let label = TrackLabel::from_metadata(&metadata);
        let requesting_user = get_requesting_user(t).await.unwrap_or(UserId::new(1));

        // No brackets around the requester: autoplay's is already "(auto)".
        // An unknown duration is left out, not shown as 00:00.
        let _ = writeln!(
            description,
            "{}. {}{} • {}",
            i + start_idx + 1,
            label.linked(INLINE_TITLE_MAX),
            duration_text(label.duration)
                .map(|d| format!(" • {d}"))
                .unwrap_or_default(),
            requesting_user_to_string(requesting_user),
        );
    }

    // An empty embed field is rejected by Discord.
    if description.is_empty() {
        return String::from(QUEUE_NO_SONGS);
    }
    description
}

/// Creates a queue embed.
pub async fn create_queue_embed(tracks: &[TrackHandle], page: usize) -> CreateEmbed<'static> {
    let (description, thumbnail): (String, String) = if !tracks.is_empty() {
        let metadata = get_track_handle_metadata(tracks.first().unwrap())
            .await
            .unwrap_or_default();

        let url = metadata.thumbnail.clone().unwrap_or_default();
        let thumbnail = match url::Url::parse(&url) {
            Ok(url) => url.to_string(),
            Err(e) => {
                tracing::error!("error parsing url: {:?}", e);
                "".to_string()
            },
        };

        let label = TrackLabel::from_metadata(&metadata);
        let description = format!(
            "{}{}",
            label.linked(INLINE_TITLE_MAX),
            duration_text(label.duration)
                .map(|d| format!(" • {d}"))
                .unwrap_or_default(),
        );
        (description, thumbnail)
    } else {
        (QUEUE_NOTHING_IS_PLAYING.to_string(), "".to_string())
    };

    CreateEmbed::default()
        .thumbnail(thumbnail, None)
        .field(QUEUE_NOW_PLAYING, Cow::Owned(description), false)
        .field(QUEUE_UP_NEXT, create_queue_page(tracks, page).await, false)
        .footer(CreateEmbedFooter::new(format!(
            "{} {} {} {}",
            QUEUE_PAGE,
            page + 1,
            QUEUE_PAGE_OF,
            calculate_num_pages(tracks),
        )))
}

// ------ NOW PLAYING ------ //
// This is probably the message that the user sees //
// the most from the bot.                         //

/// Read everything the now-playing card shows from a track. Nothing here
/// panics or hangs: missing metadata is an empty card, and a driver that does
/// not answer within `TRACK_INFO_TIMEOUT` (an offline call, a dying driver)
/// reads as "just started".
pub async fn now_playing_card(track: &TrackHandle) -> NowPlayingCard {
    let metadata = get_track_handle_metadata(track).await.unwrap_or_default();
    let requester = get_requesting_user(track).await.ok();
    let label = TrackLabel::from_metadata(&metadata);
    let progress =
        match tokio::time::timeout(crate::music::ops::TRACK_INFO_TIMEOUT, track.get_info()).await {
            Ok(Ok(info)) if info.playing == PlayMode::Pause => Progress::Paused {
                position: Some(info.position),
            },
            Ok(Ok(info)) => Progress::Playing {
                position: info.position,
                duration: label.duration,
            },
            _ => Progress::Playing {
                position: Duration::ZERO,
                duration: label.duration,
            },
        };
    NowPlayingCard {
        label,
        thumbnail: metadata.thumbnail,
        requester,
        progress,
    }
}

// ---------------------- Lyrics ---------------------------- //

/// Creates a lyrics embed for the given track.
pub async fn create_lyrics_embed_old(
    track: String,
    artists: String,
    lyric: String,
) -> CreateEmbed<'static> {
    CreateEmbed::default()
        .author(CreateEmbedAuthor::new(artists))
        .title(track)
        .description(lyric)
}
/// The author line of a lyrics embed, cut to the 255 bytes a
/// `FixedString<u8>` holds, on a character boundary. lyric_finder can credit a
/// song to dozens of artists; `from_str(..).expect` panicked on that, and the
/// command never answered.
fn lyrics_author(artists: &str) -> FixedString<u8> {
    FixedString::from_str_trunc(artists)
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
            UNKNOWN.to_string(),
            UNKNOWN.to_string(),
            "No lyrics found!".to_string(),
        ),
    };

    create_paged_embed(
        ctx,
        lyrics_author(&artists),
        track,
        lyric,
        DEFAULT_LYRICS_PAGE_SIZE,
        PagedStyle::default(),
    )
    .await
}

// ---------------------- Navigation Buttons ---------------------------- //

/// Builds a single navigation button for the queue.
pub fn create_single_nav_btn(label: &str, is_disabled: bool) -> CreateButton<'_> {
    CreateButton::new(label.to_string().to_ascii_lowercase())
        .label(label)
        .style(ButtonStyle::Primary)
        .disabled(is_disabled)
        .to_owned()
}

/// Builds the four navigation buttons for the queue.
pub fn create_nav_btns<'att>(page: usize, num_pages: usize) -> Vec<CreateComponent<'att>> {
    let (cant_left, cant_right) = (page < 1, page >= num_pages - 1);
    // serenity's components-v2 rework wraps every top level component in
    // `CreateComponent`; an action row is now one variant of that enum.
    vec![CreateComponent::ActionRow(CreateActionRow::Buttons(
        Cow::Owned(vec![
            create_single_nav_btn("<<", cant_left),
            create_single_nav_btn("<", cant_left),
            create_single_nav_btn(">", cant_right),
            create_single_nav_btn(">>", cant_right),
        ]),
    ))]
}

// -------- Search Results -------- //

/// The search results reply: a line of text, then one embed per hit, in
/// order.
#[must_use]
pub fn create_search_results_reply(results: Vec<CreateEmbed<'static>>) -> Rendered {
    let mut results = results.into_iter();
    let mut out = Rendered::text("Search results:");
    out.embed = results.next();
    out.embeds_extra = results.collect();
    out
}
/// Sends a message to the user indicating that the search failed.
pub async fn send_search_failed(ctx: &CrackContext<'_>) -> Result<(), CrackedError> {
    let _guild_id = ctx.guild_id().unwrap();
    let embed = CreateEmbed::default()
        .description(format!(
            "{}",
            CrackedError::Other("Something went wrong while parsing your query!")
        ))
        .footer(CreateEmbedFooter::new("Search failed!"));
    let _msg = send_embed_response_poise(*ctx, embed).await?;
    //ctx.data().add_msg_to_cache(guild_id, msg).await;
    Ok(())
}

/// Sends a message to the user indicating that no query was provided.
pub async fn send_no_query_provided(ctx: &CrackContext<'_>) -> Result<(), CrackedError> {
    let embed = CreateEmbed::default()
        .description(format!("{}", CrackedError::Other("No query provided!")))
        .footer(CreateEmbedFooter::new("No query provided!"));
    send_embed_response_poise(*ctx, embed).await?;
    Ok(())
}

/// Sends the searching message after a play command is sent.
#[cfg(not(tarpaulin_include))]
pub async fn send_search_message<'ctx>(
    ctx: &'ctx CrackContext<'_>,
) -> CrackedResult<ReplyHandle<'ctx>> {
    send_search_message_as(ctx, false).await
}

/// The "searching…" reply, ephemeral when `ephemeral` is set.
#[cfg(not(tarpaulin_include))]
pub async fn send_search_message_as<'ctx>(
    ctx: &'ctx CrackContext<'_>,
    ephemeral: bool,
) -> CrackedResult<ReplyHandle<'ctx>> {
    let embed = CreateEmbed::default().description(format!("{}", CrackedMessage::Search));
    let msg = crate::utils::send_embed_response_poise_as(*ctx, embed, ephemeral).await?;
    Ok(msg)
}

/// Send the search results to the user.
pub async fn create_search_response<'ctx>(
    ctx: &'ctx CrackContext<'_>,
    guild_id: GuildId,
    user_id: UserId,
    query: String,
    res: Vec<AuxMetadata>,
) -> Result<ReplyHandle<'ctx>, CrackedError> {
    let author = ctx
        .author_member()
        .await
        .ok_or(CrackedError::AuthorNotFound)?;
    let name = author.mention().to_string();

    let now_time_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let fields = build_embed_fields(res).await;
    let author = CreateEmbedAuthor::new(name);
    let title = format!("Search results for: {}", query);
    let footer = CreateEmbedFooter::new(format!("{} * {} * {}", user_id, guild_id, now_time_str));
    let embed = CreateEmbed::new()
        .author(author)
        .title(title)
        .footer(footer)
        .fields(fields.into_iter().map(|f| (f.name, f.value, f.inline)));

    send_embed_response_poise(*ctx, embed).await
}

// ---------------------- Joining Channel ---------------------------- //

use crate::poise_ext::PoiseContextExt;
/// Sends a message to the user indicating that the search failed.
pub async fn send_joining_channel<'ctx>(
    ctx: &'ctx CrackContext<'_>,
    channel_id: GenericChannelId,
) -> Result<ReplyHandle<'ctx>, Error> {
    let msg = CrackedMessage::Summon {
        mention: channel_id.mention(),
    };
    let params = SendMessageParams::new(msg).with_channel(channel_id);

    ctx.send_message(params).await.map_err(Into::into)
}

// ---------------------- Most Generic Message Function ---------------//

async fn build_embed_fields(elems: Vec<AuxMetadata>) -> Vec<EmbedField> {
    use crate::utils::duration_to_string;
    tracing::warn!("num elems: {:?}", elems.len());
    let mut fields = vec![];
    // let tmp = "".to_string();
    for elem in elems.into_iter() {
        let title = elem.title.unwrap_or_default();
        let link = elem.source_url.unwrap_or_default();
        let duration = elem.duration.unwrap_or_default();
        let elem = format!("({}) - {}", link, duration_to_string(duration));
        fields.push(EmbedField::new(format!("[{}]", title), elem, true));
    }
    fields
}

#[cfg(test)]
mod test {
    /// Every hit is in the one reply, in order, under its line of text.
    #[test]
    fn search_results_are_one_reply_with_every_hit_in_order() {
        use serenity::all::CreateEmbed;
        let out = super::create_search_results_reply(vec![
            CreateEmbed::new().title("(1)[a]"),
            CreateEmbed::new().title("(2)[b]"),
            CreateEmbed::new().title("(3)[c]"),
        ]);
        assert_eq!(out.content.as_deref(), Some("Search results:"));
        let v = serde_json::to_value(out.to_message()).unwrap();
        let titles: Vec<&str> = v["embeds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["title"].as_str().unwrap())
            .collect();
        assert_eq!(titles, ["(1)[a]", "(2)[b]", "(3)[c]"]);
    }

    /// `get_info` never answers on an offline call; the card must still come
    /// back, as "just started", within the bound.
    #[tokio::test]
    async fn a_driver_that_never_answers_does_not_hang_the_card() {
        use super::now_playing_card;
        use crate::messaging::format::Progress;
        use std::time::Duration;
        let (_data, call, _ids, _rx) = crate::music::ops::test_support::queue_of(1).await;
        let handle = call.lock().await.queue().current_queue()[0].clone();
        let card = tokio::time::timeout(Duration::from_secs(5), now_playing_card(&handle))
            .await
            .expect("now_playing_card outlived its bound");
        assert_eq!(card.label.title.as_deref(), Some("t0"));
        assert!(
            matches!(card.progress, Progress::Playing { position, .. } if position == Duration::ZERO)
        );
    }

    /// A songbird call with no connection, queued with `(title, requester)`
    /// tracks in order; `None` is an autoplayed track. The call is returned so
    /// it outlives the handles.
    async fn queue_of(
        tracks: &[(&str, Option<u64>)],
    ) -> (
        std::sync::Arc<tokio::sync::Mutex<songbird::Call>>,
        Vec<songbird::tracks::TrackHandle>,
    ) {
        use crate::music::queue::{enqueue_track_back, new_track};
        use crate::music::PlaybackOwner;
        use crate::{Data, DataInner};
        use serenity::model::id::{GuildId, UserId};

        let guild = GuildId::new(1);
        let data = Data(std::sync::Arc::new(DataInner::default()));
        let guard = data
            .lock_queue(
                guild,
                PlaybackOwner::Free,
                crate::music::audit::Actor::bot(crate::music::audit::BotReason::Autopause),
            )
            .await
            .unwrap();
        let call = std::sync::Arc::new(tokio::sync::Mutex::new(songbird::Call::standalone(
            guild,
            UserId::new(2),
        )));
        for (title, requester) in tracks {
            let metadata = songbird::input::AuxMetadata {
                title: Some((*title).to_owned()),
                ..Default::default()
            };
            let source = songbird::input::File::new("/nonexistent/queued.opus").into();
            let track = new_track(source, Some(metadata), requester.map(UserId::new));
            enqueue_track_back(&guard, &call, track, None).await;
        }
        let queued = call.lock().await.queue().current_queue();
        (call, queued)
    }

    /// "Up next" is what plays after the current track. It once started from
    /// the current track itself, so a one-song queue listed that song twice.
    #[tokio::test]
    async fn up_next_lists_only_the_tracks_after_the_one_playing() {
        let (_call, tracks) =
            queue_of(&[("I Had It All", None), ("The Old Dun Cow", Some(9))]).await;

        let page = super::create_queue_page(&tracks, 0).await;

        assert!(page.starts_with("1. **The Old Dun Cow**"), "{page}");
        assert!(!page.contains("I Had It All"), "{page}");
    }

    /// `calculate_num_pages` already counts only what is up next; page two has
    /// to carry on where page one stopped.
    #[tokio::test]
    async fn the_second_up_next_page_carries_on_from_the_first() {
        let titles: Vec<String> = (0..8).map(|i| format!("Track {i}")).collect();
        let queued: Vec<(&str, Option<u64>)> = titles.iter().map(|t| (t.as_str(), None)).collect();
        let (_call, tracks) = queue_of(&queued).await;

        let page = super::create_queue_page(&tracks, 1).await;

        assert!(page.starts_with("7. **Track 7**"), "{page}");
        assert_eq!(crate::utils::calculate_num_pages(&tracks), 2);
    }

    #[tokio::test]
    async fn up_next_with_nothing_after_the_playing_track_says_so() {
        let (_call, tracks) = queue_of(&[("I Had It All", None)]).await;

        let page = super::create_queue_page(&tracks, 0).await;

        assert_eq!(page, crate::messaging::messages::QUEUE_NO_SONGS);
    }

    /// `requesting_user_to_string` already brackets autoplay as "(auto)"; the
    /// queue line bracketed it again.
    #[tokio::test]
    async fn an_autoplayed_track_is_credited_in_one_pair_of_brackets() {
        let (_call, tracks) = queue_of(&[("I Had It All", None), ("I Had It All", None)]).await;

        let page = super::create_queue_page(&tracks, 0).await;

        assert!(page.contains("(auto)"), "{page}");
        assert!(!page.contains("((auto))"), "{page}");
    }

    /// A track nothing resolved: no title, no link, no duration. It reads
    /// `(untitled)`, links nowhere, and never claims to be 00:00 long.
    #[tokio::test]
    async fn an_untitled_track_without_link_or_duration_has_a_plain_line() {
        let (_call, tracks) = queue_of(&[("Playing", None), ("", Some(9))]).await;

        let page = super::create_queue_page(&tracks, 0).await;

        assert_eq!(page, "1. **(untitled)** • <@9>\n");
        assert!(!page.contains("00:00"), "{page}");
    }

    /// The queue title is third-party text: escaped, not trusted as markdown.
    #[tokio::test]
    async fn a_queue_line_escapes_its_title() {
        let (_call, tracks) = queue_of(&[("Playing", None), ("a*b", Some(9))]).await;

        let page = super::create_queue_page(&tracks, 0).await;

        assert!(page.contains("**a\\*b**"), "{page}");
    }

    /// The "Now playing" line follows the same rules as the lines below it.
    #[tokio::test]
    async fn an_unknown_now_playing_track_is_untitled_and_has_no_duration() {
        let (_call, tracks) = queue_of(&[("", None)]).await;

        let embed = super::create_queue_embed(&tracks, 0).await;

        let wire = serde_json::to_string(&embed).unwrap();
        assert!(wire.contains("**(untitled)**"), "{wire}");
        assert!(!wire.contains("00:00"), "{wire}");
    }

    #[test]
    fn test_requesting_user_to_string() {
        use super::requesting_user_to_string;
        use serenity::model::id::UserId;

        assert_eq!(requesting_user_to_string(UserId::new(1)), "(auto)");
        assert_eq!(requesting_user_to_string(UserId::new(2)), "<@2>");
    }

    /// 🪤 Measured on production v0.11.0: lyric_finder credited a song to
    /// dozens of artists, far past the 255 bytes a `FixedString<u8>` holds,
    /// and `from_str(..).expect("wtf?")` panicked -- Discord showed "The
    /// application did not respond".
    #[test]
    fn a_lyrics_credit_too_long_for_an_embed_author_is_cut_short_not_a_panic() {
        use super::lyrics_author;

        let credit = format!(
            "Lyrical Lemonade (Ft. {})",
            "Aesthetic (Rapper), ".repeat(20)
        );
        assert!(credit.len() > 255, "the fixture must be over the limit");
        assert_eq!(lyrics_author(&credit).as_str(), &credit[..255]);

        // Two bytes a char: byte 255 would split one, so the cut lands at 254.
        let accents = "é".repeat(200);
        assert_eq!(lyrics_author(&accents).as_str(), "é".repeat(127));

        assert_eq!(lyrics_author("The Offspring").as_str(), "The Offspring");
    }

    #[tokio::test]
    async fn test_build_log_embed() {
        let title = "title";
        let description = "description";
        let avatar_url = "avatar_url";
        let embed = super::build_log_embed(title, description, avatar_url).await;
        assert!(embed.is_ok());
        let embed = embed.unwrap();
        // Previously this exercised the unstable `std::fmt::FormattingOptions`
        // API; the plain `Debug` impl gives the same coverage on stable.
        let formatted_output = format!("{embed:?}");
        assert!(!formatted_output.is_empty());
    }
}
