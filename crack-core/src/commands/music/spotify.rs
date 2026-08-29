use crate::commands::help;
use crate::sources::spotify::{MediaType, Spotify};
use crate::{Context, Error};
use crack_sleevenote::{Album, Client as Sleevenote, Error as SleevenoteError, Playlist, Track};
use poise::CreateReply;
use serenity::all::{Color, CreateEmbed};
use std::sync::OnceLock;

/// How many tracks of a collection to list before saying "and N more".
const PREVIEW_TRACKS: usize = 10;

/// One client for the process. `from_env` builds a reqwest client, and reqwest
/// pools connections internally -- building one per invocation would discard
/// the pool every time. The init is fallible (a bad SLEEVENOTE_URL), so the
/// result is what gets cached; retrying it per call would just re-fail.
static CLIENT: OnceLock<Result<Sleevenote, String>> = OnceLock::new();

fn client() -> Result<&'static Sleevenote, String> {
    CLIENT
        .get_or_init(|| Sleevenote::from_env().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

/// Look up a Spotify track, album or playlist.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Music", prefix_command, slash_command)]
pub async fn spotify(
    ctx: Context<'_>,
    #[flag]
    #[description = "Show a help menu for this command."]
    help: bool,
    #[rest]
    #[description = "A Spotify track, album or playlist URL."]
    url: Option<String>,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    spotify_internal(ctx, url).await
}

/// A resolve can take ten seconds or more on a cold cache, which is well past
/// Discord's three-second interaction deadline -- so defer before doing any of
/// it, or the command fails before the answer exists.
#[cfg(not(tarpaulin_include))]
pub async fn spotify_internal(ctx: Context<'_>, url: Option<String>) -> Result<(), Error> {
    let Some(url) = url.filter(|u| !u.trim().is_empty()) else {
        return reply(ctx, fail("Give me a Spotify track, album or playlist URL.")).await;
    };

    let parsed = match Spotify::parse_spotify_url(&url).await {
        Ok(parsed) => parsed,
        Err(_) => {
            return reply(
                ctx,
                fail("That is not a Spotify track, album or playlist URL."),
            )
            .await
        },
    };

    let client = match client() {
        Ok(client) => client,
        Err(why) => {
            tracing::error!("sleevenote client unavailable: {why}");
            return reply(ctx, fail("Spotify lookup is not configured on this bot.")).await;
        },
    };

    ctx.defer().await?;

    let id = parsed.media_id();
    let embed = match parsed.media_type() {
        MediaType::Track => client.track(id).await.map(track_embed),
        MediaType::Album => client.album(id).await.map(album_embed),
        MediaType::Playlist => client.playlist(id).await.map(playlist_embed),
    };

    reply(ctx, embed.unwrap_or_else(|e| error_embed(&e))).await
}

/// Each arm is a different diagnosis, and saying so is the entire reason the
/// client keeps these variants apart. Collapsing them here would throw the
/// distinction away at the last possible moment: "this does not exist" and
/// "our scraper broke" call for opposite reactions from whoever reads it.
fn error_embed(err: &SleevenoteError) -> CreateEmbed<'static> {
    match err {
        SleevenoteError::NotFound(_) => fail("Spotify has nothing at that link."),
        SleevenoteError::InvalidId(_) => fail("That link does not contain a usable Spotify id."),
        SleevenoteError::Timeout(_) => {
            fail("Spotify took too long to answer. Try again in a moment.")
        },
        // Not the caller's fault and not retryable by them: the lookup service
        // stopped matching Spotify's page. Say so plainly rather than offering
        // a retry that cannot work.
        SleevenoteError::ExtractionEmpty(_) | SleevenoteError::ExtractionIncomplete(_) => {
            fail("Spotify lookup is broken right now -- this has been logged.")
        },
        other => {
            tracing::error!("sleevenote lookup failed: {other}");
            fail("Spotify lookup failed.")
        },
    }
}

fn track_embed(track: Track) -> CreateEmbed<'static> {
    let mut embed = CreateEmbed::default()
        .title(track.name.clone())
        .url(track.url.clone())
        .description(artists(&track))
        .color(Color::BLURPLE);

    if let Some(album) = &track.album {
        embed = embed.field("Album", album.name.clone(), true);
        if let Some(image) = &album.image {
            embed = embed.thumbnail(image.clone(), None);
        }
    }
    if let Some(duration) = track.duration() {
        embed = embed.field("Length", hms(duration.as_secs()), true);
    }
    embed
}

fn album_embed(album: Album) -> CreateEmbed<'static> {
    let names = album
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    collection_embed(
        album.name,
        album.url,
        names,
        &album.tracks,
        album.unresolved_items,
        album.image,
    )
}

fn playlist_embed(playlist: Playlist) -> CreateEmbed<'static> {
    collection_embed(
        playlist.name,
        playlist.url,
        playlist.owner.unwrap_or_else(|| "Spotify".to_string()),
        &playlist.tracks,
        playlist.unresolved_items,
        playlist.image,
    )
}

fn collection_embed(
    name: String,
    url: String,
    byline: String,
    tracks: &[Track],
    unresolved: u32,
    image: Option<String>,
) -> CreateEmbed<'static> {
    let mut listing = tracks
        .iter()
        .take(PREVIEW_TRACKS)
        .enumerate()
        .map(|(i, t)| format!("{}. {} — {}", i + 1, t.name, artists(t)))
        .collect::<Vec<_>>()
        .join("\n");

    if tracks.len() > PREVIEW_TRACKS {
        listing.push_str(&format!("\n…and {} more", tracks.len() - PREVIEW_TRACKS));
    }
    if listing.is_empty() {
        listing.push_str("No playable tracks.");
    }

    let mut embed = CreateEmbed::default()
        .title(name)
        .url(url)
        .description(byline)
        .field("Tracks", tracks.len().to_string(), true)
        .color(Color::BLURPLE);

    // Surfaced rather than swallowed: a caller who sees only the track count
    // cannot tell a short collection from one we could only half resolve.
    // Local files land here, and there is nothing to play for them.
    if unresolved > 0 {
        embed = embed.field("Unavailable", unresolved.to_string(), true);
    }
    if let Some(image) = image {
        embed = embed.thumbnail(image, None);
    }
    embed.field("Listing", listing, false)
}

fn artists(track: &Track) -> String {
    let names = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if names.is_empty() {
        "Unknown artist".to_string()
    } else {
        names
    }
}

fn hms(secs: u64) -> String {
    let (m, s) = (secs / 60, secs % 60);
    format!("{m}:{s:02}")
}

fn fail(msg: &str) -> CreateEmbed<'static> {
    CreateEmbed::default()
        .description(msg.to_string())
        .color(Color::RED)
}

async fn reply(ctx: Context<'_>, embed: CreateEmbed<'static>) -> Result<(), Error> {
    ctx.send(CreateReply::default().embed(embed))
        .await
        .map(|_| ())
        .map_err(Into::into)
}
