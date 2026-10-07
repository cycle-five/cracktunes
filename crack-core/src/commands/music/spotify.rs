use crate::commands::help;
use crate::messaging::format::{escape, http_url};
use crate::messaging::{courier, message::CrackedMessage};
use crate::sources::sleevenote::{self, MediaType};
use crate::{Context, Error};
use crack_sleevenote::{Album, Error as SleevenoteError, Playlist, Track};
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
        embed = embed.field("Album", escape(&album.name), true);
        // Only a web URL: an empty or relative one is left out.
        if let Some(image) = http_url(album.image.as_deref()) {
            embed = embed.thumbnail(image.to_string(), None);
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
        .map(|a| escape(&a.name))
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
        escape(&playlist.owner.unwrap_or_else(|| "Spotify".to_string())),
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
        .map(|(i, t)| format!("{}. {} — {}", i + 1, escape(&t.name), artists(t)))
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
    if let Some(image) = http_url(image.as_deref()) {
        embed = embed.thumbnail(image.to_string(), None);
    }
    embed.field("Listing", listing, false)
}

fn artists(track: &Track) -> String {
    let names = track
        .artists
        .iter()
        .map(|a| escape(&a.name))
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
    courier::reply(ctx, CrackedMessage::CreateEmbed(Box::new(embed)))
        .await
        .map(|_| ())
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hostile_track() -> Track {
        Track {
            id: "t".into(),
            tag: Default::default(),
            name: "[click](https://evil.example)".into(),
            artists: vec![crack_sleevenote::Artist {
                name: "@everyone".into(),
                id: None,
            }],
            album: None,
            duration_ms: None,
            url: "https://open.spotify.com/track/t".into(),
        }
    }

    /// Track names, artists and owners come from Spotify pages: none may
    /// become a masked link or a ping in the embed.
    #[test]
    fn a_listing_escapes_what_spotify_called_things() {
        let embed = collection_embed(
            "Mix".into(),
            "https://open.spotify.com/playlist/p".into(),
            r"*bold* owner".into(),
            &[hostile_track()],
            0,
            None,
            None,
        );
        let json = serde_json::to_string(&embed).unwrap();
        assert!(
            json.contains(r"1. \\[click\\](https://evil.example) — \\@everyone"),
            "{json}"
        );
    }

    #[test]
    fn a_track_embed_escapes_its_artists_and_album() {
        let mut track = hostile_track();
        track.album = Some(
            serde_json::from_str(
                r#"{"id":"a","name":"_album_","url":"https://open.spotify.com/album/a","image":null}"#,
            )
            .expect("album"),
        );
        let json = serde_json::to_string(&track_embed(track)).unwrap();
        assert!(json.contains(r"\\@everyone"), "{json}");
        assert!(json.contains(r"\\_album\\_"), "{json}");
    }

    fn thumbnail_of(embed: &CreateEmbed<'static>) -> Option<String> {
        let json = serde_json::to_value(embed).unwrap();
        json.get("thumbnail")
            .and_then(|t| t["url"].as_str())
            .map(str::to_owned)
    }

    /// Cover art is set only from a web URL, as on every other card: an
    /// empty or relative one is left out rather than sent broken.
    #[test]
    fn cover_art_is_set_only_from_a_web_url() {
        for (image, want) in [
            (
                "https://i.scdn.co/image/ab67",
                Some("https://i.scdn.co/image/ab67"),
            ),
            ("", None),
            ("/image/ab67", None),
        ] {
            let mut track = hostile_track();
            track.album = Some(
                serde_json::from_str(&format!(
                    r#"{{"id":"a","name":"A","url":"https://open.spotify.com/album/a","image":"{image}"}}"#
                ))
                .expect("album"),
            );
            assert_eq!(
                thumbnail_of(&track_embed(track)).as_deref(),
                want,
                "{image:?}"
            );
            let listing = collection_embed(
                "Mix".into(),
                "https://open.spotify.com/playlist/p".into(),
                "owner".into(),
                &[],
                0,
                None,
                Some(image.to_owned()),
            );
            assert_eq!(thumbnail_of(&listing).as_deref(), want, "{image:?}");
        }
    }

    #[test]
    fn a_playlist_owner_is_escaped() {
        let playlist = Playlist {
            id: "p".into(),
            tag: Default::default(),
            name: "Mix".into(),
            owner: Some("*bold*".into()),
            image: None,
            url: "https://open.spotify.com/playlist/p".into(),
            tracks: vec![],
            unresolved_items: 0,
            declared_items: None,
            complete: true,
        };
        let json = serde_json::to_string(&playlist_embed(playlist)).unwrap();
        assert!(json.contains(r"\\*bold\\*"), "{json}");
    }
}
