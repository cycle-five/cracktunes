//! Fixtures for tests of the game, here and in crack-core's glue: a started game, a
//! submitted song, a seeded rng. Built for `cfg(test)` and the `test-support` feature.

use super::*;
use crate::gp_prompts::{GpCategories, GpCategory, GpPrompt};
use ::serenity::all::{ChannelId, GenericChannelId, GuildId, UserId};
use crack_music::music::ops::test_support::test_music_data;
use crack_testing::ResolvedTrack;
use crack_types::{AuxMetadata, QueryType};
use rand::{rngs::StdRng, SeedableRng};
use std::sync::Arc;

pub const G: GuildId = GuildId::new(1);
pub const VC: ChannelId = ChannelId::new(10);
pub const TC: GenericChannelId = GenericChannelId::new(20);
pub const A: UserId = UserId::new(100);
pub const B: UserId = UserId::new(200);
pub const C: UserId = UserId::new(300);
/// Never submits anything, so never a player.
pub const D: UserId = UserId::new(400);
pub const NOW: i64 = 1_700_000_000;
pub const TIMER: u64 = 120;

pub fn data() -> GpData {
    GpData {
        gp_games: Arc::default(),
        gp_persist: None,
        music: test_music_data(),
    }
}

pub fn track(title: &str) -> ResolvedTrack<'static> {
    ResolvedTrack::new(QueryType::VideoLink(format!(
        "https://www.youtube.com/watch?v={title}"
    )))
    .with_metadata(AuxMetadata {
        title: Some(title.to_string()),
        source_url: Some(format!("https://www.youtube.com/watch?v={title}")),
        ..Default::default()
    })
}

pub fn rng() -> StdRng {
    StdRng::seed_from_u64(0)
}

pub fn prompts(names: &[&str]) -> Vec<GpPrompt> {
    names
        .iter()
        .map(|s| GpPrompt {
            category: GpCategory::Nostalgia,
            text: s.to_string(),
        })
        .collect()
}

pub fn nostalgia() -> GpCategories {
    GpCategories::from_choice(GpCategory::Nostalgia).unwrap()
}

/// A game hosted by alice with the given prompts; round 0's window is open.
pub fn game_with(data: &GpData, prompt_list: &[&str]) -> GpWindowOpened {
    game_with_clip(data, prompt_list, None)
}

/// As [`game_with`], with an explicit clip setting. The reveal is the
/// game's default -- held to the round's end -- so the tests below exercise
/// the game as it is played; one about per-song reveals opts into
/// [`GpReveal::Song`] through [`game_with_reveal`].
pub fn game_with_clip(data: &GpData, prompt_list: &[&str], clip: Option<GpClip>) -> GpWindowOpened {
    game_with_reveal(data, prompt_list, clip, GpReveal::default())
}

/// As [`game_with_clip`], with an explicit reveal setting.
pub fn game_with_reveal(
    data: &GpData,
    prompt_list: &[&str],
    clip: Option<GpClip>,
    reveal: GpReveal,
) -> GpWindowOpened {
    game_with_settings(data, prompt_list, clip, reveal, true)
}

/// Every setting spelled out.
pub fn game_with_settings(
    data: &GpData,
    prompt_list: &[&str],
    clip: Option<GpClip>,
    reveal: GpReveal,
    round_results: bool,
) -> GpWindowOpened {
    data.gp_start(
        G,
        A,
        "alice".into(),
        VC,
        TC,
        nostalgia(),
        prompts(prompt_list),
        TIMER,
        clip,
        reveal,
        round_results,
        NOW,
    )
    .unwrap()
}

/// A one-round game started at `at` instead of [`NOW`]: a game's identity is
/// its guild and its start, so a second game in the same guild needs another.
pub fn game_started_at(data: &GpData, at: i64) -> GpWindowOpened {
    data.gp_start(
        G,
        A,
        "alice".into(),
        VC,
        TC,
        nostalgia(),
        prompts(&["later"]),
        TIMER,
        None,
        GpReveal::default(),
        true,
        at,
    )
    .unwrap()
}

pub fn submit(data: &GpData, user: UserId, name: &str, title: &str) -> GpSubmitOutcome {
    data.gp_submit(G, user, name.into(), track(title), &[])
        .unwrap()
}

pub fn game(data: &GpData) -> GpGame {
    data.gp_games.get(&G).unwrap().clone()
}

/// A one-round game whose only song is `title`, closed and so playing.
pub fn playing_with_title(data: &GpData, title: &str, reveal: GpReveal) -> GpTrackStart {
    let opened = game_with_reveal(data, &["only"], None, reveal);
    submit(data, B, "bob", title);
    let closed = data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    match closed.next {
        GpNext::Track(start) => *start,
        other => panic!("expected a song, got {other:?}"),
    }
}
