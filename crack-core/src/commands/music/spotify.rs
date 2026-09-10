use crate::commands::help;
use crate::sources::sleevenote::{self, MediaType};
use crate::{Context, Error};
use crack_sleevenote::{Album, Error as SleevenoteError, Playlist, Track};
use poise::CreateReply;
use serenity::all::{Color, CreateEmbed};

/// How many tracks of a collection to list before saying "and N more".
const PREVIEW_TRACKS: usize = 10;

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

    let Some(parsed) = sleevenote::parse_link(&url).await else {
        return reply(
            ctx,
            fail("That is not a Spotify track, album or playlist URL."),
        )
        .await;
    };

    let client = match sleevenote::client() {
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

/// Each sleevenote failure keeps its own message, which is the entire reason
/// the client keeps those variants apart. The mapping itself lives with the
/// resolver so that a lookup and a play report the same failure the same way
/// -- they used to word the same diagnosis differently.
fn error_embed(err: &SleevenoteError) -> CreateEmbed<'static> {
    fail(sleevenote::user_message(err))
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
    // Read before the fields below are moved out of `album`.
    let shortfall = album.shortfall();
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
        shortfall,
        album.image,
    )
}

fn playlist_embed(playlist: Playlist) -> CreateEmbed<'static> {
    // Read before the fields below are moved out of `playlist`.
    let shortfall = playlist.shortfall();
    collection_embed(
        playlist.name,
        playlist.url,
        playlist.owner.unwrap_or_else(|| "Spotify".to_string()),
        &playlist.tracks,
        playlist.unresolved_items,
        shortfall,
        playlist.image,
    )
}

fn collection_embed(
    name: String,
    url: String,
    byline: String,
    tracks: &[Track],
    unresolved: u32,
    shortfall: Option<u64>,
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
    // A second, unrelated way to come up short: items Spotify declared that the
    // scrape never reached. "Unavailable" counts things we saw and could not
    // use; this counts things we never saw. A listing can have both, or either.
    if let Some(missing) = shortfall.filter(|n| *n > 0) {
        embed = embed.field("Not recovered", missing.to_string(), true);
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
