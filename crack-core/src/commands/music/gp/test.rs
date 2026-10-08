use crate::commands::music::gp_prompts::GpCategory;
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::{
    GP_FOOLED_EVERYONE, GP_FULL_SONG, GP_GAME_OVER, GP_GUESSED_RIGHT, GP_LIKES, GP_LIKE_HINT,
    GP_LIKE_LABEL, GP_NOBODY_GUESSED, GP_NOBODY_YET, GP_PROMPT_CLOSES_TITLE,
    GP_PROMPT_HOW_TO_TITLE, GP_RESULTS_GUESSED_BY, GP_RESULTS_GUESSED_COUNT,
    GP_RESULTS_NOBODY_SCORED, GP_RESULTS_THIS_ROUND, GP_RESULTS_TITLE, GP_REVEAL, GP_REVEAL_HELD,
    GP_ROUND_HINT, GP_ROUND_TITLE, GP_SCOREBOARD, GP_SELECT_PLACEHOLDER, GP_SONG_TITLE,
    GP_STATUS_PLAYING, GP_STATUS_SUBMITTING, GP_TRACK_FAILED, GP_TRACK_FAILED_NOTE,
    GP_WINDOW_CLOSED, GP_WINDOW_CLOSED_SONGS, GP_WINDOW_EMPTY, GP_WINDOW_WARNING,
};
use crate::music::PlaybackOwner;
use crate::{errors::CrackedError, Data};
use ::serenity::all::{ChannelId, GenericChannelId, GuildId, MessageId, UserId};
use crack_testing::ResolvedTrack;
use crack_types::QueryType;
use std::{collections::HashMap, sync::Arc, time::Duration};

use super::commands::refuse_gp;
use super::playback::never_played;
use super::*;
use crate::DataInner;
use crack_types::AuxMetadata;
use rand::{rngs::StdRng, SeedableRng};

const G: GuildId = GuildId::new(1);
const VC: ChannelId = ChannelId::new(10);
const TC: GenericChannelId = GenericChannelId::new(20);
const A: UserId = UserId::new(100);
const B: UserId = UserId::new(200);
const C: UserId = UserId::new(300);
/// Never submits anything, so never a player.
const D: UserId = UserId::new(400);
const NOW: i64 = 1_700_000_000;
const TIMER: u64 = 120;

fn data() -> Data {
    Data(Arc::new(DataInner {
        ..Default::default()
    }))
}

fn track(title: &str) -> ResolvedTrack<'static> {
    ResolvedTrack::new(QueryType::VideoLink(format!(
        "https://www.youtube.com/watch?v={title}"
    )))
    .with_metadata(AuxMetadata {
        title: Some(title.to_string()),
        source_url: Some(format!("https://www.youtube.com/watch?v={title}")),
        ..Default::default()
    })
}

fn rng() -> StdRng {
    StdRng::seed_from_u64(0)
}

fn prompts(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

/// A clip that never fires in the pure-state tests, but proves the setting is
/// carried from `/gp start` all the way to `GpTrackStart`.
fn clip() -> GpClip {
    GpClip {
        start: Duration::from_secs(GP_DEFAULT_CLIP_START_SECS),
        length: Duration::from_secs(GP_DEFAULT_CLIP_LENGTH_SECS),
    }
}

/// A game hosted by alice with the given prompts; round 0's window is open.
fn game_with(data: &Data, prompt_list: &[&str]) -> GpWindowOpened {
    game_with_clip(data, prompt_list, None)
}

/// As [`game_with`], with an explicit clip setting. The reveal is the
/// game's default -- held to the round's end -- so the tests below exercise
/// the game as it is played; one about per-song reveals opts into
/// [`GpReveal::Song`] through [`game_with_reveal`].
fn game_with_clip(data: &Data, prompt_list: &[&str], clip: Option<GpClip>) -> GpWindowOpened {
    game_with_reveal(data, prompt_list, clip, GpReveal::default())
}

/// As [`game_with_clip`], with an explicit reveal setting.
fn game_with_reveal(
    data: &Data,
    prompt_list: &[&str],
    clip: Option<GpClip>,
    reveal: GpReveal,
) -> GpWindowOpened {
    game_with_settings(data, prompt_list, clip, reveal, true)
}

/// Every setting spelled out.
fn game_with_settings(
    data: &Data,
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
        GpCategory::Nostalgia,
        prompts(prompt_list),
        TIMER,
        clip,
        reveal,
        round_results,
        NOW,
    )
    .unwrap()
}

fn submit(data: &Data, user: UserId, name: &str, title: &str) -> GpSubmitOutcome {
    data.gp_submit(G, user, name.into(), track(title), &[])
        .unwrap()
}

fn game(data: &Data) -> GpGame {
    data.gp_games.get(&G).unwrap().clone()
}

#[test]
fn start_opens_round_one() {
    let data = data();
    assert_eq!(
        data.gp_submit(G, A, "a".into(), track("x"), &[])
            .unwrap_err(),
        CrackedError::NoGameInProgress
    );
    let opened = game_with(&data, &["p1", "p2"]);
    assert_eq!((opened.round_idx, opened.total_rounds), (0, 2));
    assert_eq!(opened.prompt, "p1");
    assert_eq!(opened.closes_at, NOW + TIMER as i64);
    assert_eq!(opened.generation, 1);
    assert_eq!(opened.text_channel, TC);
    let g = game(&data);
    assert_eq!(g.phase, GpPhase::Submitting);
    assert_eq!(g.rounds[0].closes_at, Some(NOW + TIMER as i64));
    // The game owns playback from the start; there is no lobby.
    assert!(data.gp_is_active(G));
    assert_eq!(data.gp_voice_channel(G), Some(VC));
    assert!(data.gp_window_open(G).is_ok());
    assert_eq!(
        data.gp_start(
            G,
            B,
            "bob".into(),
            VC,
            TC,
            GpCategory::Mixed,
            prompts(&["x"]),
            TIMER,
            None,
            GpReveal::Song,
            true,
            NOW
        )
        .unwrap_err(),
        CrackedError::GameAlreadyRunning
    );
    assert_eq!(
        data.gp_start(
            GuildId::new(2),
            B,
            "bob".into(),
            VC,
            TC,
            GpCategory::Mixed,
            vec![],
            TIMER,
            None,
            GpReveal::Song,
            true,
            NOW
        )
        .unwrap_err(),
        CrackedError::Other("That category has no prompts.")
    );
}

#[test]
fn resubmit_replaces() {
    let data = data();
    game_with(&data, &["p1"]);
    let first = submit(&data, B, "bob", "one");
    assert_eq!(
        first,
        GpSubmitOutcome {
            replaced: false,
            submitted: 1,
            everyone_in: false,
            generation: 1
        }
    );
    let second = submit(&data, B, "bob", "two");
    assert!(second.replaced);
    assert_eq!(second.submitted, 1);
    let g = game(&data);
    assert_eq!(g.rounds[0].submissions.len(), 1);
    assert_eq!(g.rounds[0].submissions[&B].get_title(), "two");
    assert_eq!(g.players.get(&B).map(String::as_str), Some("bob"));
}

#[test]
fn everyone_in_is_set_based() {
    let data = data();
    game_with(&data, &["p1"]);
    // Nobody known in the VC (cache miss): never closes early.
    assert!(!submit(&data, A, "alice", "a").everyone_in);
    // Bob is in the VC and hasn't submitted.
    let out = data
        .gp_submit(G, A, "alice".into(), track("a2"), &[A, B])
        .unwrap();
    assert!(!out.everyone_in);
    // Bob submits; carol (a leaver) isn't in the VC list, so she doesn't block.
    let out = data
        .gp_submit(G, B, "bob".into(), track("b"), &[A, B])
        .unwrap();
    assert!(out.everyone_in);
    // A newcomer who hasn't submitted blocks again.
    let out = data
        .gp_submit(G, B, "bob".into(), track("b2"), &[A, B, C])
        .unwrap();
    assert!(!out.everyone_in);
}

#[test]
fn too_many_players_per_round() {
    let data = data();
    game_with(&data, &["p1"]);
    for i in 0..GP_MAX_PLAYERS as u64 {
        submit(&data, UserId::new(1000 + i), &format!("u{i}"), "t");
    }
    // An existing submitter may still swap their song...
    assert!(submit(&data, UserId::new(1000), "u0", "t2").replaced);
    // ...but a 26th distinct submitter may not.
    assert_eq!(
        data.gp_submit(G, UserId::new(5000), "new".into(), track("t"), &[])
            .unwrap_err(),
        CrackedError::TooManyPlayers(GP_MAX_PLAYERS)
    );
}

#[test]
fn close_shuffles_and_plays() {
    let data = data();
    game_with(&data, &["p1", "p2"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");
    let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    assert_eq!((closed.round_idx, closed.total_rounds), (0, 2));
    assert_eq!(closed.prompt, "p1");
    assert_eq!(closed.count, 3);
    let GpNext::Track(start) = &closed.next else {
        panic!("expected a track, got {:?}", closed.next);
    };
    assert_eq!(
        (start.round_idx, start.track_idx, start.total_tracks),
        (0, 0, 3)
    );
    assert!(start.guessable);
    assert_eq!(start.prompt, "p1");
    assert_eq!(
        start.players,
        vec![
            (A, "alice".to_string()),
            (B, "bob".to_string()),
            (C, "carol".to_string())
        ]
    );
    let g = game(&data);
    assert_eq!(g.phase, GpPhase::Playing);
    assert_eq!(g.generation, 2);
    assert!(g.rounds[0].submissions.is_empty());
    assert_eq!(g.rounds[0].closes_at, None);
    let mut submitters: Vec<UserId> = g.rounds[0].tracks.iter().map(|t| t.submitter).collect();
    submitters.sort_unstable();
    assert_eq!(submitters, vec![A, B, C]);
    // Seeded shuffles are reproducible.
    let data2 = data_with_same_round();
    let closed2 = data2.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let GpNext::Track(start2) = closed2.next else {
        unreachable!()
    };
    assert_eq!(start2.track.get_title(), start.track.get_title());
    // Submissions are closed now.
    assert_eq!(
        data.gp_submit(G, A, "alice".into(), track("late"), &[])
            .unwrap_err(),
        CrackedError::WindowClosed
    );
    assert_eq!(
        data.gp_window_open(G).unwrap_err(),
        CrackedError::WindowClosed
    );
}

fn data_with_same_round() -> Data {
    let data = data();
    game_with(&data, &["p1", "p2"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");
    data
}

#[test]
fn close_zero_submissions_skips_to_next_window() {
    let data = data();
    game_with(&data, &["p1", "p2"]);
    let closed = data.gp_close_window(G, A, &mut rng(), NOW + 5).unwrap();
    assert_eq!(closed.count, 0);
    let GpNext::Window(opened) = &closed.next else {
        panic!("expected next window, got {:?}", closed.next);
    };
    assert_eq!(opened.round_idx, 1);
    assert_eq!(opened.prompt, "p2");
    assert_eq!(opened.closes_at, NOW + 5 + TIMER as i64);
    let g = game(&data);
    assert_eq!(g.phase, GpPhase::Submitting);
    assert_eq!(g.current_round, 1);
}

#[test]
fn close_zero_submissions_finishes_on_last_round() {
    let data = data();
    game_with(&data, &["only"]);
    let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    assert!(matches!(closed.next, GpNext::Finished(_)));
    let g = game(&data);
    assert_eq!(g.phase, GpPhase::Finished);
    assert!(data.gp_is_active(G));
    // No window to close any more.
    assert_eq!(
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap_err(),
        CrackedError::WindowClosed
    );
    data.gp_remove(G);
    assert!(!data.gp_is_active(G));
}

#[test]
fn close_if_generation_guard() {
    let data = data();
    let opened = game_with(&data, &["p1", "p2"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    // A timer from a generation that never existed does nothing.
    assert!(data.gp_close_window_if(G, 99, &mut rng(), NOW).is_none());
    assert!(data.gp_warning_if(G, 99).is_none());
    // The right generation warns and closes.
    let w = data.gp_warning_if(G, opened.generation).unwrap();
    assert_eq!(
        (w.round_idx, w.count, w.closes_at),
        (0, 2, opened.closes_at)
    );
    assert_eq!(w.prompt, "p1");
    let closed = data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    assert_eq!(closed.count, 2);
    // The same timer firing again (or the host) is a no-op / error now.
    assert!(data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .is_none());
    assert!(data.gp_warning_if(G, opened.generation).is_none());
    assert_eq!(
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap_err(),
        CrackedError::WindowClosed
    );
    // No game at all.
    assert!(data
        .gp_close_window_if(GuildId::new(9), 1, &mut rng(), NOW)
        .is_none());
}

#[test]
fn host_close_permissions() {
    let data = data();
    game_with(&data, &["p1"]);
    assert_eq!(
        data.gp_close_window(G, B, &mut rng(), NOW).unwrap_err(),
        CrackedError::NotGameHost
    );
    assert_eq!(
        data.gp_close_window(GuildId::new(9), A, &mut rng(), NOW)
            .unwrap_err(),
        CrackedError::NoGameInProgress
    );
}

#[test]
fn single_submission_is_likes_only() {
    let data = data();
    // Round 0 gets both of them in, so bob is a player for round 1 even
    // though he sits that one out.
    game_with(&data, &["p1", "p2"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();

    submit(&data, A, "alice", "a2");
    let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let GpNext::Track(start) = &closed.next else {
        unreachable!()
    };
    assert!(!start.guessable);
    assert_eq!(start.total_tracks, 1);
    // No dropdown, so no guessing -- even for a player.
    assert_eq!(
        data.gp_record_guess(G, 1, 0, B, "bob".into(), A)
            .unwrap_err(),
        CrackedError::NotGuessable
    );
    // Watching is still not playing.
    assert_eq!(
        data.gp_toggle_like(G, 1, 0, D, "dave".into()).unwrap_err(),
        CrackedError::NotAGamePlayer
    );
    // ...but a player's likes still count.
    assert_eq!(
        data.gp_toggle_like(G, 1, 0, B, "bob".into()).unwrap(),
        GpLikeOutcome::Liked(1)
    );
    let before = game(&data).scores.get(&A).copied().unwrap_or(0);
    let res = data.gp_reveal_and_advance(G, 1, 0, NOW).unwrap();
    assert!(!res.guessable);
    assert!(!res.fooled_everyone);
    assert!(res.correct.is_empty());
    assert_eq!(res.likes, 1);
    assert_eq!(
        game(&data).scores.get(&A),
        Some(&(before + GP_POINTS_PER_LIKE))
    );
    assert!(matches!(res.next, GpNext::Finished(_)));
}

/// Submitting is what makes someone a player, and it sticks for the rest of
/// the game: sitting a round out does not put them back outside it.
#[test]
fn membership_is_earned_by_submitting_and_sticks() {
    let data = data();
    game_with(&data, &["p1", "p2"]);
    // The host counts before submitting so they can watch their own game...
    assert!(data.gp_require_player(G, A).is_ok());
    // ...but nobody else does.
    assert_eq!(
        data.gp_require_player(G, B).unwrap_err(),
        CrackedError::NotAGamePlayer
    );
    submit(&data, B, "bob", "b");
    assert!(data.gp_require_player(G, B).is_ok());
    assert_eq!(
        data.gp_require_player(G, D).unwrap_err(),
        CrackedError::NotAGamePlayer
    );

    // Play round 0 out; bob submits nothing in round 1 but stays a player.
    submit(&data, A, "alice", "a");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
    assert_eq!(game(&data).current_round, 1);
    assert!(data.gp_require_player(G, B).is_ok());
    assert_eq!(
        data.gp_require_player(G, D).unwrap_err(),
        CrackedError::NotAGamePlayer
    );
}

#[test]
fn votes_required_is_a_majority() {
    // Strict majority of the eligible voters (everyone in the channel bar the
    // song's submitter), so one player can never skip for the whole room.
    assert_eq!(gp_votes_required(4), 3);
    assert_eq!(gp_votes_required(3), 2);
    assert_eq!(gp_votes_required(2), 2);
    assert_eq!(gp_votes_required(1), 1);
    // An empty or uncached voice channel must not make zero votes enough.
    assert_eq!(gp_votes_required(0), 1);
}

#[test]
fn vote_skip_needs_a_majority() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");

    // Nothing is playing yet.
    assert_eq!(
        data.gp_vote_skip(G, A, "alice".into(), &[A, B, C])
            .unwrap_err(),
        CrackedError::GameNotPlaying
    );

    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let vc = [A, B, C];
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    let voters: Vec<UserId> = vc.iter().copied().filter(|u| *u != s0).collect();

    // Watching is not playing.
    assert_eq!(
        data.gp_vote_skip(G, D, "dave".into(), &vc).unwrap_err(),
        CrackedError::NotAGamePlayer
    );
    // The submitter does not vote on their own song -- they pull it, and the
    // pull is not recorded as a vote.
    assert_eq!(
        data.gp_vote_skip(G, s0, "self".into(), &vc).unwrap(),
        GpVoteSkipOutcome::OwnSong
    );
    assert!(game(&data).rounds[0].tracks[0].skip_votes.is_empty());
    // Pulling is idempotent: it never trips the already-voted guard.
    assert_eq!(
        data.gp_vote_skip(G, s0, "self".into(), &vc).unwrap(),
        GpVoteSkipOutcome::OwnSong
    );

    // The submitter is out of the pool, so the other two carry the vote.
    assert_eq!(
        data.gp_vote_skip(G, voters[0], "v0".into(), &vc).unwrap(),
        GpVoteSkipOutcome::Counted {
            votes: 1,
            needed: 1
        }
    );
    // Voting twice does not carry the vote.
    assert_eq!(
        data.gp_vote_skip(G, voters[0], "v0".into(), &vc)
            .unwrap_err(),
        CrackedError::AlreadyVotedSkip
    );
    assert_eq!(
        data.gp_vote_skip(G, voters[1], "v1".into(), &vc).unwrap(),
        GpVoteSkipOutcome::Passed
    );

    // Votes belong to the song, so the next one starts clean.
    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    assert!(matches!(res.next, GpNext::Track(_)));
    assert!(game(&data).rounds[0].tracks[1].skip_votes.is_empty());
    let s1 = game(&data).rounds[0].tracks[1].submitter;
    let next_voter = vc.iter().copied().find(|u| *u != s1).unwrap();
    assert_eq!(
        data.gp_vote_skip(G, next_voter, "v".into(), &vc).unwrap(),
        GpVoteSkipOutcome::Counted {
            votes: 1,
            needed: 1
        }
    );
}

/// `/gp end` leaves the game in the map on purpose: `stop()` only queues the
/// `End`, so removing it there would beat the event and hand it to autoplay.
/// Collection is the track-end handler's job, and only a *parked* game is its
/// to collect.
#[test]
fn parked_game_is_collected_by_the_track_end_and_not_before() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();

    // A game that is merely running is not anyone's to collect.
    assert!(!data.gp_remove_if_parked(G));
    assert!(data.gp_is_active(G));

    data.gp_park_for_end(G, A, false).unwrap();
    // Still present, which is the whole point -- the global handler has to see
    // a game when the End that stop() queued finally lands.
    assert!(data.gp_is_active(G));
    // ...and inert, so the game's own handlers do not reveal or advance on it.
    assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());

    assert!(data.gp_remove_if_parked(G));
    assert!(!data.gp_is_active(G));
    // Idempotent: a second End, or the command's backstop, finds nothing.
    assert!(!data.gp_remove_if_parked(G));
}

/// A dead link and a stream that dies part-way through are not the same thing.
/// Only the first reached nobody, and only the first should skip the scoring.
#[test]
fn never_played_needs_errored_and_too_little_play_time() {
    use songbird::tracks::{PlayMode, TrackState};
    let errored = |play_time| TrackState {
        playing: PlayMode::Errored(songbird::tracks::PlayError::Create(Arc::new(
            songbird::input::AudioStreamError::Unsupported,
        ))),
        play_time,
        ..Default::default()
    };
    // Preparing -> Errored without mixing a frame: nobody heard it.
    assert!(never_played(&errored(Duration::ZERO), None));
    // Died 200ms in: a dead link as far as the room is concerned. Scoring it
    // would hand the submitter the fooled-everyone bonus for a song nobody
    // could have guessed.
    assert!(never_played(&errored(Duration::from_millis(200)), None));
    // Either side of the line.
    assert!(never_played(
        &errored(GP_MIN_PLAYED - Duration::from_millis(1)),
        None
    ));
    assert!(!never_played(&errored(GP_MIN_PLAYED), None));
    // Died two minutes in: the room heard it, so it scores like any other song.
    assert!(!never_played(&errored(Duration::from_secs(120)), None));
    // A 45s clip is judged against its own length, not the flat thirty: it
    // needs 22.5s, so 25s counts as heard where a whole song would not.
    let clip45 = Some(Duration::from_secs(45));
    assert!(!never_played(&errored(Duration::from_secs(25)), clip45));
    assert!(never_played(&errored(Duration::from_secs(20)), clip45));
    // A song that simply finished is not a failure at any play time.
    assert!(!never_played(
        &TrackState {
            playing: PlayMode::End,
            play_time: Duration::ZERO,
            ..Default::default()
        },
        None
    ));
    assert!(!never_played(&TrackState::default(), None));
}

/// A vote to hear more of a song is first-hand evidence it was playing, and
/// outranks whatever the play time says afterwards. A skip vote is not: a dead
/// link is silent, and silence is what makes people reach for `/gp voteskip`.
#[test]
fn a_full_song_vote_beats_the_played_threshold() {
    let data = data();
    game_with_clip(&data, &["p1"], Some(clip()));
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let vc = [A, B, C];
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    let voter = vc.iter().copied().find(|u| *u != s0).unwrap();

    assert!(!data.gp_heard_by_vote(G, 0, 0));

    // A skip vote is not evidence of anything being audible.
    let skipper = vc
        .iter()
        .copied()
        .find(|u| *u != s0 && *u != voter)
        .unwrap();
    data.gp_vote_skip(G, skipper, "s".into(), &vc).unwrap();
    assert!(!data.gp_heard_by_vote(G, 0, 0));

    // One vote for more of it is, even before the vote carries.
    assert_eq!(
        data.gp_vote_full(G, voter, "v".into(), &vc).unwrap(),
        GpVoteFullOutcome::Counted {
            votes: 1,
            needed: 1
        }
    );
    assert!(data.gp_heard_by_vote(G, 0, 0));
    assert!(!data.gp_plays_full(G, 0, 0), "not carried yet");

    // Out of range, and an absent game, are not evidence either.
    assert!(!data.gp_heard_by_vote(G, 0, 99));
    assert!(!data.gp_heard_by_vote(GuildId::new(9), 0, 0));
}

/// A submitter gets the same answer whether or not the song has already been
/// voted up: it is never theirs to vote on.
#[test]
fn vote_full_tells_the_submitter_the_same_thing_either_way() {
    let data = data();
    game_with_clip(&data, &["p1"], Some(clip()));
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let vc = [A, B, C];
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    let voters: Vec<UserId> = vc.iter().copied().filter(|u| *u != s0).collect();

    assert_eq!(
        data.gp_vote_full(G, s0, "self".into(), &vc).unwrap_err(),
        CrackedError::CannotVoteOwnSongFull
    );
    // Carry it, then ask again: still their own song, still the same answer.
    data.gp_vote_full(G, voters[0], "v0".into(), &vc).unwrap();
    data.gp_vote_full(G, voters[1], "v1".into(), &vc).unwrap();
    assert!(data.gp_plays_full(G, 0, 0));
    assert_eq!(
        data.gp_vote_full(G, s0, "self".into(), &vc).unwrap_err(),
        CrackedError::CannotVoteOwnSongFull
    );
}

/// A clip has to fit the song. Seeking past the end comes back as an immediate
/// `End`, which the game would read as a dead link and score nobody for.
#[test]
fn clip_fits_itself_to_the_song() {
    let c = GpClip {
        start: Duration::from_secs(30),
        length: Duration::from_secs(45),
    };
    let secs = |s| Some(Duration::from_secs(s));

    // Comfortably long enough: untouched.
    assert_eq!(c.for_duration(secs(240)), c);
    // Exactly long enough: still untouched.
    assert_eq!(c.for_duration(secs(75)), c);
    // Unknown duration: trust the offset, nothing better to go on.
    assert_eq!(c.for_duration(None), c);

    // Too short for start+length: take the last `length` instead of seeking
    // past the end.
    assert_eq!(
        c.for_duration(secs(60)),
        GpClip {
            start: Duration::from_secs(15),
            length: Duration::from_secs(45)
        }
    );
    // Shorter than the clip itself: play all of it, from the top.
    assert_eq!(
        c.for_duration(secs(20)),
        GpClip {
            start: Duration::ZERO,
            length: Duration::from_secs(20)
        }
    );
}

/// The "did the room hear it" bar has to scale with what was meant to play, or
/// a clip that ran to its end is scored as a dead link.
#[test]
fn min_played_scales_with_the_intended_length() {
    // A whole song keeps the flat thirty seconds.
    assert_eq!(gp_min_played(Some(Duration::from_secs(240))), GP_MIN_PLAYED);
    assert_eq!(gp_min_played(None), GP_MIN_PLAYED);
    // A 45s clip needs 22.5s, not 30 -- which would be most of the clip.
    assert_eq!(
        gp_min_played(Some(Duration::from_secs(45))),
        Duration::from_millis(22_500)
    );
    // A 20s clip needs 10s. Under the old absolute rule it could never clear
    // the bar at all, so every short clip scored nobody.
    assert_eq!(
        gp_min_played(Some(Duration::from_secs(20))),
        Duration::from_secs(10)
    );
}

/// Voting a song up to full length: same pool as voteskip, but the submitter
/// is barred outright rather than given a pull -- it is their own bonus.
#[test]
fn vote_full_carries_on_a_majority() {
    let data = data();
    game_with_clip(&data, &["p1"], Some(clip()));
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let vc = [A, B, C];
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    let voters: Vec<UserId> = vc.iter().copied().filter(|u| *u != s0).collect();

    assert!(!data.gp_plays_full(G, 0, 0));
    // Not a player, and not the submitter's to vote for.
    assert_eq!(
        data.gp_vote_full(G, D, "dave".into(), &vc).unwrap_err(),
        CrackedError::NotAGamePlayer
    );
    assert_eq!(
        data.gp_vote_full(G, s0, "self".into(), &vc).unwrap_err(),
        CrackedError::CannotVoteOwnSongFull
    );

    assert_eq!(
        data.gp_vote_full(G, voters[0], "v0".into(), &vc).unwrap(),
        GpVoteFullOutcome::Counted {
            votes: 1,
            needed: 1
        }
    );
    assert_eq!(
        data.gp_vote_full(G, voters[0], "v0".into(), &vc)
            .unwrap_err(),
        CrackedError::AlreadyVotedFull
    );
    assert!(!data.gp_plays_full(G, 0, 0));

    assert_eq!(
        data.gp_vote_full(G, voters[1], "v1".into(), &vc).unwrap(),
        GpVoteFullOutcome::Passed
    );
    // The clip timer asks this before it stops anything.
    assert!(data.gp_plays_full(G, 0, 0));
    assert_eq!(
        data.gp_vote_full(G, voters[0], "v0".into(), &vc).unwrap(),
        GpVoteFullOutcome::AlreadyFull
    );

    // The submitter is paid for it at the reveal.
    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    assert!(res.played_full);
    let scored = game(&data).scores.get(&s0).copied().unwrap_or(0);
    assert!(
        scored >= GP_POINTS_FULL_SONG,
        "submitter should have the full-song bonus, got {scored}"
    );
}

/// A game already playing whole songs has nothing to vote up.
#[test]
fn vote_full_needs_a_game_playing_clips() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    assert_eq!(
        data.gp_vote_full(G, A, "alice".into(), &[A, B])
            .unwrap_err(),
        CrackedError::NotPlayingClips
    );
}

/// The clip setting has to survive from `/gp start` to the song that plays.
#[test]
fn clip_setting_reaches_the_track() {
    let data = data();
    game_with_clip(&data, &["p1"], Some(clip()));
    submit(&data, A, "alice", "a");
    let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let GpNext::Track(start) = &closed.next else {
        unreachable!()
    };
    assert_eq!(start.clip, Some(clip()));
    // The timer keys off this, the same way the window timer does.
    assert_eq!(start.generation, game(&data).generation);
    assert!(data.gp_clip_still_current(G, start.generation, 0, 0));
    assert!(!data.gp_clip_still_current(G, start.generation + 1, 0, 0));
    assert!(!data.gp_clip_still_current(G, start.generation, 0, 1));
}

/// The pool the majority is measured against has to be the people who may
/// actually vote. Counting a lurker sets a bar the eligible voters cannot
/// clear, and the song becomes unskippable -- in exactly the mixed channel
/// that submitters-only voting creates.
#[test]
fn vote_skip_pool_counts_players_not_bystanders() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    let other = if s0 == A { B } else { A };

    // Three in the channel, two of them players, the song belongs to one of
    // those two: D cannot vote, so the one eligible voter has to be enough.
    assert_eq!(
        data.gp_vote_skip(G, D, "dave".into(), &[A, B, D])
            .unwrap_err(),
        CrackedError::NotAGamePlayer
    );
    assert_eq!(
        data.gp_vote_skip(G, other, "other".into(), &[A, B, D])
            .unwrap(),
        GpVoteSkipOutcome::Passed
    );
}

/// Excluding the submitter from the pool is what keeps a song skippable: a
/// two-person channel would otherwise need two votes with only one eligible
/// voter, and the song could never be voted off.
#[test]
fn vote_skip_excludes_the_submitter_from_the_pool() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    let other = if s0 == A { B } else { A };
    // Two in the channel, one of them the submitter: the other one decides.
    assert_eq!(
        data.gp_vote_skip(G, other, "other".into(), &[A, B])
            .unwrap(),
        GpVoteSkipOutcome::Passed
    );
}

#[test]
fn guesses_likes_and_scoring() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let g = game(&data);
    let s0 = g.rounds[0].tracks[0].submitter;
    let other = if s0 == A { B } else { A };

    // Wrong round / track are stale.
    assert_eq!(
        data.gp_record_guess(G, 1, 0, other, "other".into(), A)
            .unwrap_err(),
        CrackedError::StaleRound
    );
    assert_eq!(
        data.gp_record_guess(G, 0, 1, other, "other".into(), A)
            .unwrap_err(),
        CrackedError::StaleRound
    );
    assert_eq!(
        data.gp_toggle_like(G, 0, 1, other, "other".into())
            .unwrap_err(),
        CrackedError::StaleRound
    );
    // Watching is not playing: no guessing and no 👍 without a song in.
    assert_eq!(
        data.gp_record_guess(G, 0, 0, D, "dave".into(), s0)
            .unwrap_err(),
        CrackedError::NotAGamePlayer
    );
    assert_eq!(
        data.gp_toggle_like(G, 0, 0, D, "dave".into()).unwrap_err(),
        CrackedError::NotAGamePlayer
    );
    // Only submitters are valid answers.
    assert_eq!(
        data.gp_record_guess(G, 0, 0, other, "other".into(), D)
            .unwrap_err(),
        CrackedError::NotAPlayer
    );
    // A guess can be changed until the song ends.
    assert_eq!(
        data.gp_record_guess(G, 0, 0, other, "other".into(), other)
            .unwrap(),
        GpGuessOutcome::Recorded
    );
    assert_eq!(
        data.gp_record_guess(G, 0, 0, other, "other".into(), s0)
            .unwrap(),
        GpGuessOutcome::Changed
    );
    assert_eq!(
        data.gp_record_guess(G, 0, 0, other, "other".into(), s0)
            .unwrap(),
        GpGuessOutcome::Recorded
    );
    // The submitter may pick on their own song; it never scores.
    data.gp_record_guess(G, 0, 0, s0, "self".into(), s0)
        .unwrap();
    // Likes: own song rejected, everyone else toggles.
    assert_eq!(
        data.gp_toggle_like(G, 0, 0, s0, "self".into()).unwrap_err(),
        CrackedError::CannotLikeOwnSong
    );
    assert_eq!(
        data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
        GpLikeOutcome::Liked(1)
    );
    assert_eq!(
        data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
        GpLikeOutcome::Unliked(0)
    );
    assert_eq!(
        data.gp_toggle_like(G, 0, 0, other, "other".into()).unwrap(),
        GpLikeOutcome::Liked(1)
    );

    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    assert_eq!(res.submitter, s0);
    assert_eq!(res.correct, vec![other]);
    assert!(!res.fooled_everyone);
    assert_eq!(res.likes, 1);
    assert!(res.guessable);
    assert!(matches!(res.next, GpNext::Track(_)));
    let g = game(&data);
    assert_eq!(g.scores.get(&other), Some(&GP_POINTS_CORRECT));
    assert_eq!(g.scores.get(&s0), Some(&GP_POINTS_PER_LIKE));
    assert_eq!(g.current_track, 1);

    // Second call for the same song is a no-op.
    assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());
    // Guessing on the finished song is stale now.
    assert_eq!(
        data.gp_record_guess(G, 0, 0, other, "other".into(), s0)
            .unwrap_err(),
        CrackedError::StaleRound
    );

    // Song 2: nobody guesses -> fooled everyone, game finishes.
    let s1 = g.rounds[0].tracks[1].submitter;
    let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
    assert!(res.fooled_everyone);
    assert_eq!(res.likes, 0);
    assert!(matches!(res.next, GpNext::Finished(_)));
    let g = game(&data);
    assert_eq!(g.phase, GpPhase::Finished);
    assert_ne!(s1, s0);
    assert_eq!(
        g.scores.get(&s1),
        Some(&(GP_POINTS_CORRECT + GP_POINTS_FOOLED_ALL))
    );
    assert_eq!(
        data.gp_record_guess(G, 0, 1, other, "other".into(), A)
            .unwrap_err(),
        CrackedError::GameNotPlaying
    );
    assert_eq!(
        data.gp_toggle_like(G, 0, 1, other, "other".into())
            .unwrap_err(),
        CrackedError::GameNotPlaying
    );
}

/// A song that never played must not be scored. songbird reports a stream
/// it could not open as an `End` whose state is still `Errored`, and before
/// this the game happily revealed it and paid out guesses and likes for a
/// song nobody had heard.
#[test]
fn failed_track_scores_nothing_and_advances() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let submitter = game(&data).rounds[0].tracks[0].submitter;
    let guesser = if submitter == A { B } else { A };

    // A correct guess and a like, both of which would normally pay out.
    data.gp_record_guess(G, 0, 0, guesser, "guesser".into(), submitter)
        .unwrap();
    data.gp_toggle_like(G, 0, 0, guesser, "guesser".into())
        .unwrap();

    let res = data.gp_fail_and_advance(G, 0, 0, NOW).unwrap();
    assert!(res.failed);
    assert!(res.correct.is_empty(), "a correct guess must not count");
    assert!(!res.fooled_everyone, "an unheard song fools nobody");
    assert!(res.scores.iter().all(|(_, points)| *points == 0));
    // The game still moves on to the next song.
    assert!(matches!(res.next, GpNext::Track(ref s) if s.track_idx == 1));

    // The surviving song scores normally, so only the failure is skipped.
    let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
    assert!(!res.failed);
    assert!(res.scores.iter().any(|(_, points)| *points > 0));
}

/// The reveal for a failed song says so, and drops the guess/like fields
/// rather than reporting zeroes for a song that never played.
#[test]
fn failed_reveal_embed_json() {
    let data = data();
    game_with_reveal(&data, &["p1"], None, GpReveal::Song);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let res = data.gp_fail_and_advance(G, 0, 0, NOW).unwrap();

    let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 2, "failure note and scoreboard only");
    assert_eq!(fields[0]["name"], GP_TRACK_FAILED);
    assert_eq!(fields[0]["value"], GP_TRACK_FAILED_NOTE);
    assert_eq!(fields[1]["name"], GP_SCOREBOARD);
    let names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&GP_GUESSED_RIGHT), "{names:?}");
    assert!(!names.contains(&GP_LIKES), "{names:?}");
}

#[test]
fn tracks_then_next_prompt() {
    let data = data();
    game_with(&data, &["p1", "p2"]);
    for round in 0..2 {
        submit(&data, A, "alice", &format!("a{round}"));
        submit(&data, B, "bob", &format!("b{round}"));
        let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        assert!(matches!(closed.next, GpNext::Track(_)));
        let first = data.gp_reveal_and_advance(G, round, 0, NOW).unwrap();
        assert!(matches!(first.next, GpNext::Track(ref s) if s.track_idx == 1));
        let second = data.gp_reveal_and_advance(G, round, 1, NOW).unwrap();
        if round == 0 {
            assert!(
                matches!(second.next, GpNext::Window(ref w) if w.round_idx == 1 && w.prompt == "p2")
            );
            assert_eq!(game(&data).phase, GpPhase::Submitting);
        } else {
            let GpNext::Finished(scores) = second.next else {
                panic!("expected finished");
            };
            assert_eq!(scores.len(), 2);
        }
    }
    assert_eq!(game(&data).phase, GpPhase::Finished);
}

#[test]
fn end_permissions_and_missing_game() {
    let data = data();
    game_with(&data, &["p1"]);
    assert_eq!(
        data.gp_park_for_end(G, B, false).unwrap_err(),
        CrackedError::NotGameHost
    );
    // Parking hands back the game as it was, but leaves it in the map so the
    // caller can stop playback before the global handler stops seeing a game.
    let g = data.gp_park_for_end(G, C, true).unwrap();
    assert_eq!(g.host, A);
    assert_eq!(g.phase, GpPhase::Submitting);
    assert!(data.gp_is_active(G));
    // Parked, the game's own handlers and timers are already inert.
    assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());
    assert!(data
        .gp_close_window_if(G, g.generation, &mut rng(), NOW)
        .is_none());
    assert!(data.gp_warning_if(G, g.generation).is_none());

    assert!(data.gp_remove(G).is_some());
    assert!(!data.gp_is_active(G));
    assert_eq!(
        data.gp_park_for_end(G, A, false).unwrap_err(),
        CrackedError::NoGameInProgress
    );
    assert_eq!(
        data.gp_status(G).unwrap_err(),
        CrackedError::NoGameInProgress
    );
    assert_eq!(
        data.gp_window_open(G).unwrap_err(),
        CrackedError::NoGameInProgress
    );
    assert!(data.gp_remove(G).is_none());
    assert!(data.gp_reveal_and_advance(G, 0, 0, NOW).is_none());
    assert_eq!(data.gp_voice_channel(G), None);
    assert_eq!(
        data.gp_record_guess(G, 0, 0, A, "a".into(), B).unwrap_err(),
        CrackedError::NoGameInProgress
    );
    assert_eq!(
        data.gp_toggle_like(G, 0, 0, A, "a".into()).unwrap_err(),
        CrackedError::NoGameInProgress
    );
    assert_eq!(
        data.gp_set_prompt_message(G, 0, TC, MessageId::new(1))
            .unwrap_err(),
        CrackedError::NoGameInProgress
    );
    assert!(!data.gp_is_active(G));
}

#[test]
fn message_bookkeeping() {
    let data = data();
    game_with(&data, &["p1"]);
    assert_eq!(
        data.gp_set_prompt_message(G, 3, TC, MessageId::new(1))
            .unwrap_err(),
        CrackedError::StaleRound
    );
    data.gp_set_prompt_message(G, 0, TC, MessageId::new(7))
        .unwrap();
    assert_eq!(
        data.gp_set_track_message(G, 0, 0, TC, MessageId::new(1))
            .unwrap_err(),
        CrackedError::StaleRound,
        "no tracks before the window closes"
    );
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    let closed = data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    assert_eq!(closed.prompt_message, Some((TC, MessageId::new(7))));
    data.gp_set_track_message(G, 0, 0, TC, MessageId::new(42))
        .unwrap();
    assert_eq!(
        data.gp_set_track_message(G, 0, 5, TC, MessageId::new(1))
            .unwrap_err(),
        CrackedError::StaleRound
    );
    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    assert_eq!(res.message, Some((TC, MessageId::new(42))));
    assert_eq!(res.text_channel, TC);
    let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
    assert_eq!(res.message, None);
}

#[test]
fn scores_tie_break_by_name() {
    let data = data();
    game_with(&data, &["p1"]);
    submit(&data, B, "bob", "b");
    submit(&data, A, "alice", "a");
    submit(&data, C, "carol", "c");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    // Carol guesses every song but her own right, so alice and bob are
    // never fooled and tie at 0; carol takes 100 a guess plus the fooled
    // bonus for the song nobody pinned on her.
    for i in 0..3 {
        let s = game(&data).rounds[0].tracks[i].submitter;
        if s != C {
            data.gp_record_guess(G, 0, i, C, "carol".into(), s).unwrap();
        }
        data.gp_reveal_and_advance(G, 0, i, NOW).unwrap();
    }
    assert_eq!(
        game(&data).sorted_scores(),
        vec![
            (C, 2 * GP_POINTS_CORRECT + GP_POINTS_FOOLED_ALL),
            (A, 0),
            (B, 0)
        ]
    );
}

#[test]
fn status_snapshots() {
    let data = data();
    game_with(&data, &["p1", "p2"]);
    submit(&data, B, "bob", "b");
    match data.gp_status(G).unwrap() {
        GpStatus::Submitting {
            host,
            round,
            total,
            prompt,
            closes_at,
            submitted,
            scores,
        } => {
            assert_eq!(host, A);
            assert_eq!((round, total), (1, 2));
            assert_eq!(prompt, "p1");
            assert_eq!(closes_at, NOW + TIMER as i64);
            assert_eq!(submitted, vec!["bob".to_string()]);
            assert_eq!(scores.len(), 2);
        },
        other => panic!("expected submitting, got {other:?}"),
    }
    submit(&data, A, "alice", "a");
    submit(&data, C, "carol", "c");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    // Carol is a player, so she may guess and 👍 -- unless it is her song.
    let liker = if s0 == C { A } else { C };
    data.gp_record_guess(G, 0, 0, liker, "carol".into(), A)
        .unwrap();
    data.gp_toggle_like(G, 0, 0, liker, "carol".into()).unwrap();
    match data.gp_status(G).unwrap() {
        GpStatus::Playing {
            round,
            total,
            track,
            tracks,
            prompt,
            guessed,
            likes,
            scores,
        } => {
            assert_eq!((round, total, track, tracks), (1, 2, 1, 3));
            assert_eq!(prompt, "p1");
            assert_eq!(guessed, vec!["carol".to_string()]);
            assert_eq!(likes, 1);
            assert_eq!(scores.len(), 3);
        },
        other => panic!("expected playing, got {other:?}"),
    }
}

/// Play the current round out with no guesses, so the game moves on to the
/// next window (or finishes). Returns the play order it had.
fn play_out(data: &Data, round_idx: usize) -> Vec<UserId> {
    let order: Vec<UserId> = game(data).rounds[round_idx]
        .tracks
        .iter()
        .map(|t| t.submitter)
        .collect();
    for i in 0..order.len() {
        data.gp_reveal_and_advance(G, round_idx, i, NOW).unwrap();
    }
    order
}

/// The order of one round must never repeat the order of the round before
/// it -- stronger, nobody may keep the slot they had -- or the position in
/// the round says whose song it is before a note has played (#450).
#[test]
fn nobody_keeps_their_slot_from_one_round_to_the_next() {
    let players = [(A, "alice"), (B, "bob"), (C, "carol")];
    for seed in 0..40u64 {
        let data = data();
        game_with(&data, &["p0", "p1", "p2", "p3"]);
        let mut rng = StdRng::seed_from_u64(seed);
        let mut previous: Option<Vec<UserId>> = None;
        for round in 0..4 {
            for (id, name) in players {
                submit(&data, id, name, &format!("{name}{round}"));
            }
            data.gp_close_window(G, A, &mut rng, NOW).unwrap();
            let order = play_out(&data, round);
            if let Some(prev) = &previous {
                for (slot, (now, then)) in order.iter().zip(prev).enumerate() {
                    assert_ne!(
                        now, then,
                        "seed {seed}, round {round}: slot {slot} kept between rounds"
                    );
                }
            }
            previous = Some(order);
        }
    }
}

/// A round nobody submitted to has no order; the constraint reaches back
/// past it to the last round that was actually played.
#[test]
fn the_previous_order_skips_an_empty_round() {
    for seed in 0..40u64 {
        let data = data();
        game_with(&data, &["p0", "p1", "p2"]);
        let mut rng = StdRng::seed_from_u64(seed);
        for (id, name) in [(A, "alice"), (B, "bob"), (C, "carol")] {
            submit(&data, id, name, name);
        }
        data.gp_close_window(G, A, &mut rng, NOW).unwrap();
        let first = play_out(&data, 0);
        // Round 1: nobody submits, straight on to round 2's window.
        let closed = data.gp_close_window(G, A, &mut rng, NOW).unwrap();
        assert!(matches!(closed.next, GpNext::Window(_)));
        for (id, name) in [(A, "alice"), (B, "bob"), (C, "carol")] {
            submit(&data, id, name, name);
        }
        data.gp_close_window(G, A, &mut rng, NOW).unwrap();
        let third: Vec<UserId> = game(&data).rounds[2]
            .tracks
            .iter()
            .map(|t| t.submitter)
            .collect();
        assert!(
            first.iter().zip(&third).all(|(a, b)| a != b),
            "seed {seed}: {first:?} then {third:?}"
        );
    }
}

/// Two songs have two orders, and forbidding the repeat would leave one:
/// the rounds would alternate, which is a tell of its own. So two-song
/// rounds are a plain shuffle, and over enough seeds one of them repeats.
#[test]
fn two_song_rounds_are_not_deranged() {
    let mut repeats = 0;
    for seed in 0..40u64 {
        let data = data();
        game_with(&data, &["p0", "p1"]);
        let mut rng = StdRng::seed_from_u64(seed);
        submit(&data, A, "alice", "a0");
        submit(&data, B, "bob", "b0");
        data.gp_close_window(G, A, &mut rng, NOW).unwrap();
        let first = play_out(&data, 0);
        submit(&data, A, "alice", "a1");
        submit(&data, B, "bob", "b1");
        data.gp_close_window(G, A, &mut rng, NOW).unwrap();
        let second: Vec<UserId> = game(&data).rounds[1]
            .tracks
            .iter()
            .map(|t| t.submitter)
            .collect();
        if first == second {
            repeats += 1;
        }
    }
    assert!(repeats > 0, "a two-song round should be free to repeat");
    assert!(repeats < 40, "and free not to");
}

/// The constraint only reaches as far as the previous order does: slots the
/// previous round did not have, and players who were not in it, are free.
#[test]
fn shuffle_against_constrains_only_the_slots_it_can() {
    let mut rng = rng();
    let ids: Vec<UserId> = (1..=5).map(UserId::new).collect();
    // A shorter previous round: only its slots are constrained.
    for _ in 0..200 {
        let mut items: Vec<(UserId, ())> = ids.iter().map(|id| (*id, ())).collect();
        shuffle_against(&mut items, &ids[..2], &mut rng);
        assert_ne!(items[0].0, ids[0]);
        assert_ne!(items[1].0, ids[1]);
    }
    // A previous round of strangers constrains nothing, and every order is
    // still reachable.
    let strangers: Vec<UserId> = (100..=104).map(UserId::new).collect();
    let mut items: Vec<(UserId, ())> = ids.iter().map(|id| (*id, ())).collect();
    shuffle_against(&mut items, &strangers, &mut rng);
    let mut seen: Vec<UserId> = items.iter().map(|(id, _)| *id).collect();
    seen.sort_unstable();
    assert_eq!(seen, ids);
    // No previous round at all: a plain shuffle.
    let mut items: Vec<(UserId, ())> = ids.iter().map(|id| (*id, ())).collect();
    shuffle_against(&mut items, &[], &mut rng);
    assert_eq!(items.len(), 5);
    // Fewer than the minimum: unconstrained even against the same players.
    let mut same = 0;
    for _ in 0..100 {
        let mut items: Vec<(UserId, ())> = ids[..2].iter().map(|id| (*id, ())).collect();
        shuffle_against(&mut items, &ids[..2], &mut rng);
        if items[0].0 == ids[0] {
            same += 1;
        }
    }
    assert!(same > 0 && same < 100);
}

/// One scoring rule, derived from the song as it stands, so the reveal, the
/// round's results and the held-back scoreboard cannot disagree.
#[test]
fn a_song_scores_from_what_the_room_did_to_it() {
    let mut t = GpTrack::new(A, track("a"));
    // Nothing happened: nobody guessed, so the submitter fooled everyone.
    assert_eq!(
        t.score(true),
        GpTrackScore {
            correct: vec![],
            fooled_everyone: true,
            points: vec![(A, GP_POINTS_FOOLED_ALL)],
        }
    );
    // Two right guesses (in a stable order), one wrong, the submitter's own
    // pick ignored; two likes; voted up to full length.
    t.guesses.insert(C, A);
    t.guesses.insert(B, A);
    t.guesses.insert(D, B);
    t.guesses.insert(A, A);
    t.likes.insert(B);
    t.likes.insert(C);
    t.play_full = true;
    assert_eq!(
        t.score(true),
        GpTrackScore {
            correct: vec![B, C],
            fooled_everyone: false,
            points: vec![
                (B, GP_POINTS_CORRECT),
                (C, GP_POINTS_CORRECT),
                (A, 2 * GP_POINTS_PER_LIKE + GP_POINTS_FULL_SONG),
            ],
        }
    );
    // A one-song round: guesses and the fooled bonus are off, the rest stands.
    assert_eq!(
        t.score(false),
        GpTrackScore {
            correct: vec![],
            fooled_everyone: false,
            points: vec![(A, 2 * GP_POINTS_PER_LIKE + GP_POINTS_FULL_SONG)],
        }
    );
    // A song that never played pays nobody, whatever was cast on it.
    t.failed = true;
    assert_eq!(t.score(true), GpTrackScore::default());
}

/// The round's last reveal carries the round summed up: every song, what
/// the round paid, and the board after it. Earlier reveals carry nothing.
/// Per-song reveals here, so each song's reveal can be checked as it goes;
/// the held reveal's results are covered by `a_held_reveal_*` below.
#[test]
fn the_last_song_of_a_round_carries_the_rounds_results() {
    let data = data();
    game_with_reveal(&data, &["p1", "p2"], None, GpReveal::Song);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let order: Vec<UserId> = game(&data).rounds[0]
        .tracks
        .iter()
        .map(|t| t.submitter)
        .collect();
    let before = game(&data).sorted_scores();

    // Song 0: everyone else guesses it, one like. Song 1: nobody does.
    // Song 2: never played.
    for u in [A, B, C].into_iter().filter(|u| *u != order[0]) {
        data.gp_record_guess(G, 0, 0, u, "n".into(), order[0])
            .unwrap();
    }
    let liker = [A, B, C].into_iter().find(|u| *u != order[0]).unwrap();
    data.gp_toggle_like(G, 0, 0, liker, "n".into()).unwrap();
    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    assert!(res.round.is_none(), "not the last song");
    assert!(!res.held);
    let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
    assert!(res.round.is_none());
    let res = data.gp_fail_and_advance(G, 0, 2, NOW).unwrap();
    let round = res.round.expect("the last song carries the results");
    assert!(matches!(res.next, GpNext::Window(_)));

    assert_eq!((round.round_idx, round.total_rounds), (0, 2));
    assert_eq!(round.prompt, "p1");
    assert!(round.guessable);
    assert_eq!(round.songs.len(), 3);
    let s0 = &round.songs[0];
    assert_eq!(s0.submitter, order[0]);
    let mut guessers: Vec<UserId> = [A, B, C].into_iter().filter(|u| *u != order[0]).collect();
    guessers.sort_unstable();
    assert_eq!(s0.correct, guessers);
    assert!(!s0.fooled_everyone);
    assert_eq!(s0.likes, 1);
    assert!(!s0.failed);
    let s1 = &round.songs[1];
    assert_eq!(s1.submitter, order[1]);
    assert!(s1.correct.is_empty());
    assert!(s1.fooled_everyone);
    let s2 = &round.songs[2];
    assert!(s2.failed);
    assert!(!s2.fooled_everyone, "an unheard song fools nobody");

    // What the round paid is exactly the change in the totals.
    let after = game(&data).sorted_scores();
    let paid: HashMap<UserId, u32> = round.points.iter().copied().collect();
    for (id, total) in &after {
        let was = before
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, p)| *p)
            .unwrap_or(0);
        assert_eq!(total - was, paid.get(id).copied().unwrap_or(0), "{id}");
    }
    assert!(
        round.points.iter().all(|(_, p)| *p > 0),
        "only players who took something"
    );
    assert_eq!(round.points[0].1, GP_POINTS_CORRECT + GP_POINTS_FOOLED_ALL);
    assert_eq!(round.scores, after);

    // The last round's results come with the finish.
    submit(&data, A, "alice", "a2");
    submit(&data, B, "bob", "b2");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    data.gp_reveal_and_advance(G, 1, 0, NOW).unwrap();
    let res = data.gp_reveal_and_advance(G, 1, 1, NOW).unwrap();
    assert!(matches!(res.next, GpNext::Finished(_)));
    assert_eq!(res.round.unwrap().round_idx, 1);
}

/// With the reveal held to the end of the round, a song's end names nobody
/// and moves no visible score: the totals still carry the round's payout,
/// and showing them would say who guessed right and who fooled the room.
#[test]
fn a_held_reveal_names_nobody_and_moves_no_visible_score() {
    let data = data();
    game_with_reveal(&data, &["p1", "p2"], None, GpReveal::Round);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    submit(&data, C, "carol", "c");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let s0 = game(&data).rounds[0].tracks[0].submitter;
    let guesser = [A, B, C].into_iter().find(|u| *u != s0).unwrap();
    data.gp_record_guess(G, 0, 0, guesser, "n".into(), s0)
        .unwrap();
    data.gp_toggle_like(G, 0, 0, guesser, "n".into()).unwrap();

    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    assert!(res.held);
    assert!(res.scores.iter().all(|(_, p)| *p == 0), "{:?}", res.scores);
    let g = game(&data);
    assert!(g.scores.values().sum::<u32>() > 0, "paid, just not shown");
    assert!(g.visible_scores().iter().all(|(_, p)| *p == 0));
    assert!(g.sorted_scores().iter().any(|(_, p)| *p > 0));
    // `/gp status` shows the same held-back board.
    let GpStatus::Playing { scores, .. } = data.gp_status(G).unwrap() else {
        panic!("playing");
    };
    assert!(scores.iter().all(|(_, p)| *p == 0));

    let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
    let desc = v["description"].as_str().unwrap();
    assert!(desc.contains(GP_REVEAL_HELD), "{desc}");
    assert!(!desc.contains("<@"), "must name nobody: {desc}");
    assert!(!desc.contains(GP_REVEAL), "{desc}");
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 1, "likes only, no scoreboard: {fields:?}");
    assert_eq!(fields[0]["name"], GP_LIKES);
    assert_eq!(fields[0]["value"], "1");

    // A song that never played, held: says so, still names nobody.
    let res = data.gp_fail_and_advance(G, 0, 1, NOW).unwrap();
    assert!(res.held && res.failed);
    let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
    assert!(!serde_json::to_string(&v).unwrap().contains("<@"));
    assert_eq!(v["fields"][0]["name"], GP_TRACK_FAILED);

    // The round's end is the reveal: the results carry names and totals, and
    // from then on the board is whole again.
    let res = data.gp_reveal_and_advance(G, 0, 2, NOW).unwrap();
    let round = res.round.unwrap();
    assert!(round.scores.iter().any(|(_, p)| *p > 0));
    assert_eq!(round.scores, game(&data).sorted_scores());
    assert_eq!(game(&data).visible_scores(), game(&data).sorted_scores());
    let GpStatus::Submitting { scores, .. } = data.gp_status(G).unwrap() else {
        panic!("submitting");
    };
    assert!(scores.iter().any(|(_, p)| *p > 0));
}

/// A host may turn the round's results off and have the game as it was:
/// each song's reveal and nothing summing them up. Not with the reveal held
/// to the round's end, though -- then the results are the reveal.
#[test]
fn round_results_can_be_turned_off_unless_they_are_the_reveal() {
    for (reveal, expect_results) in [(GpReveal::Song, false), (GpReveal::Round, true)] {
        let data = data();
        game_with_settings(&data, &["p1"], None, reveal, false);
        submit(&data, A, "alice", "a");
        submit(&data, B, "bob", "b");
        data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
        data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
        let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
        assert!(matches!(res.next, GpNext::Finished(_)));
        assert_eq!(res.round.is_some(), expect_results, "{reveal:?}");
    }
}

/// In the default game the visible board is the board; nothing is held.
#[test]
fn the_default_reveal_is_held_to_the_round() {
    // `/gp start` without `reveal:` is `unwrap_or_default()`; the tests'
    // `game_with` starts the same game.
    assert_eq!(GpReveal::default(), GpReveal::Round);
    let data = data();
    game_with(&data, &["p1"]);
    assert_eq!(game(&data).reveal, GpReveal::Round);
}

#[test]
fn a_round_owes_its_results_until_they_are_marked_posted() {
    let data = data();
    game_with(&data, &["p1", "p2"]);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    // Mid-round nothing is owed: the round has not ended.
    assert!(game(&data).unposted_results().is_empty());
    let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
    assert!(res.round.is_some(), "the round's results go out now");
    // The snapshot has moved the game on; until the post is marked, a
    // resume would owe round 0 its results.
    assert_eq!(game(&data).current_round, 1);
    assert_eq!(game(&data).unposted_results(), vec![0]);
    data.gp_mark_results_posted(G, 0);
    assert!(game(&data).rounds[0].results_posted);
    assert!(game(&data).unposted_results().is_empty());
    // Out of range is ignored, not a panic.
    data.gp_mark_results_posted(G, 9);
    data.gp_mark_results_posted(GuildId::new(2), 0);
}

#[test]
fn a_game_without_results_and_a_skipped_round_owe_nothing() {
    // Results off and a per-song reveal: there is no embed to owe.
    let data = data();
    game_with_settings(&data, &["p1", "p2"], None, GpReveal::Song, false);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    let res = data.gp_reveal_and_advance(G, 0, 1, NOW).unwrap();
    assert!(res.round.is_none());
    assert!(game(&data).unposted_results().is_empty());

    // A round nobody submitted to ends at the close with nothing to sum up.
    let empty = self::data();
    game_with(&empty, &["p1", "p2"]);
    let closed = empty.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    assert!(matches!(closed.next, GpNext::Window(_)));
    assert_eq!(game(&empty).current_round, 1);
    assert!(game(&empty).unposted_results().is_empty());
}

#[test]
fn a_song_reveal_shows_the_running_total() {
    let data = data();
    game_with_reveal(&data, &["p1"], None, GpReveal::Song);
    submit(&data, A, "alice", "a");
    submit(&data, B, "bob", "b");
    data.gp_close_window(G, A, &mut rng(), NOW).unwrap();
    let res = data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
    assert!(!res.held);
    assert!(res.scores.iter().any(|(_, p)| *p > 0), "fooled everyone");
    assert_eq!(res.scores, game(&data).sorted_scores());
    assert_eq!(game(&data).visible_scores(), game(&data).sorted_scores());
}

#[test]
fn round_results_embed_json() {
    let song = |submitter, title: &str, correct: Vec<UserId>| GpSongResult {
        submitter,
        title: title.into(),
        correct,
        fooled_everyone: false,
        likes: 2,
        played_full: false,
        failed: false,
    };
    let r = GpRoundResult {
        round_idx: 1,
        total_rounds: 5,
        prompt: "Cry song.".into(),
        guessable: true,
        songs: vec![
            song(A, "one", vec![B, C]),
            GpSongResult {
                fooled_everyone: true,
                played_full: true,
                ..song(B, "two", vec![])
            },
            GpSongResult {
                failed: true,
                ..song(C, "three", vec![A])
            },
        ],
        points: vec![(B, 250), (C, 100), (A, 20)],
        scores: vec![(B, 400), (A, 300), (C, 100)],
    };
    let v = serde_json::to_value(gp_round_results_embed(&r)).unwrap();
    assert_eq!(
        v["title"],
        format!("{GP_ROUND_TITLE} 2/5 {GP_RESULTS_TITLE}")
    );
    let desc = v["description"].as_str().unwrap();
    let lines: Vec<&str> = desc.lines().collect();
    assert_eq!(lines[0], "**Cry song.**");
    assert_eq!(
        lines[2],
        format!("1. **one** · <@100> · {GP_RESULTS_GUESSED_BY} <@200>, <@300> · 👍 2")
    );
    assert_eq!(
        lines[3],
        format!(
            "2. **two** · <@200> · {GP_RESULTS_GUESSED_BY} {GP_NOBODY_GUESSED} · 👍 2 · {GP_FOOLED_EVERYONE} · {GP_FULL_SONG}"
        )
    );
    assert_eq!(
        lines[4],
        format!("3. **three** · <@300> · {GP_TRACK_FAILED}")
    );
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0]["name"], GP_RESULTS_THIS_ROUND);
    assert_eq!(
        fields[0]["value"],
        "1. <@200> — +250\n2. <@300> — +100\n3. <@100> — +20"
    );
    assert_eq!(fields[1]["name"], GP_SCOREBOARD);
    assert_eq!(
        fields[1]["value"],
        "1. <@200> — 400\n2. <@100> — 300\n3. <@300> — 100"
    );

    // A one-song round has no guessing to report, and nobody may have scored.
    let solo = GpRoundResult {
        guessable: false,
        songs: vec![song(A, "only", vec![])],
        points: vec![],
        ..r.clone()
    };
    let v = serde_json::to_value(gp_round_results_embed(&solo)).unwrap();
    let desc = v["description"].as_str().unwrap();
    assert!(!desc.contains(GP_RESULTS_GUESSED_BY), "{desc}");
    assert!(desc.contains("**only** · <@100> · 👍 2"), "{desc}");
    assert_eq!(v["fields"][0]["value"], GP_RESULTS_NOBODY_SCORED);

    // A full room: 25 songs each guessed by the other 24 is more mentions
    // than a description holds, so the guessers are counted instead.
    let ids: Vec<UserId> = (1..=25).map(|i| UserId::new(1_000_000_000 + i)).collect();
    let big = GpRoundResult {
        songs: ids
            .iter()
            .map(|id| {
                song(
                    *id,
                    "a song with a fairly long title",
                    ids.iter().copied().filter(|o| o != id).collect(),
                )
            })
            .collect(),
        ..r
    };
    let v = serde_json::to_value(gp_round_results_embed(&big)).unwrap();
    let desc = v["description"].as_str().unwrap();
    assert!(
        desc.chars().count() <= GP_EMBED_DESCRIPTION_MAX,
        "{}",
        desc.len()
    );
    assert!(
        desc.contains(&format!("24 {GP_RESULTS_GUESSED_COUNT}")),
        "{desc}"
    );
    assert_eq!(desc.lines().count(), 27, "every song is still there");
}

#[test]
fn custom_ids() {
    assert_eq!(
        parse_custom_id("gp:g:1:0:0"),
        Some((GpComponent::Guess, GuildId::new(1), 0, 0))
    );
    assert_eq!(
        parse_custom_id("gp:l:1:2:7"),
        Some((GpComponent::Like, GuildId::new(1), 2, 7))
    );
    assert_eq!(
        parse_custom_id(&gp_custom_id(GpComponent::Guess, G, 3, 4)),
        Some((GpComponent::Guess, G, 3, 4))
    );
    assert_eq!(parse_custom_id("gp:x:1:0:0"), None);
    assert_eq!(parse_custom_id("gp:g:0:0:0"), None);
    assert_eq!(parse_custom_id("gp:g:1:0"), None);
    assert_eq!(parse_custom_id("gp:g:1:0:0:9"), None);
    assert_eq!(parse_custom_id("gp:1:0"), None, "the v1 shape is rejected");
    assert_eq!(parse_custom_id("song_select"), None);
}

/// The controls as Discord will receive them: a string select (only when
/// guessable) and a 👍 button, each in its own action row.
#[test]
fn components_json() {
    let players = vec![(B, "bob".to_string()), (A, "alice".to_string())];
    let rows = gp_components(G, 1, 2, &players, true);
    assert_eq!(rows.len(), 2);
    let v = serde_json::to_value(&rows).unwrap();
    let menu = &v[0]["components"][0];
    assert_eq!(menu["custom_id"], "gp:g:1:1:2");
    assert_eq!(menu["placeholder"], GP_SELECT_PLACEHOLDER);
    assert_eq!(menu["min_values"], 1);
    assert_eq!(menu["max_values"], 1);
    let options = menu["options"].as_array().unwrap();
    assert_eq!(options.len(), 2);
    assert_eq!(options[0]["label"], "bob");
    assert_eq!(options[0]["value"], "200");
    let button = &v[1]["components"][0];
    assert_eq!(button["custom_id"], "gp:l:1:1:2");
    assert_eq!(button["label"], GP_LIKE_LABEL);
    assert_eq!(button["emoji"]["name"], "👍");
    assert_eq!(button["style"], 2, "secondary");

    // Not guessable: only the like button.
    let rows = gp_components(G, 0, 0, &players, false);
    assert_eq!(rows.len(), 1);
    let v = serde_json::to_value(&rows).unwrap();
    assert_eq!(v[0]["components"][0]["custom_id"], "gp:l:1:0:0");

    // Options are capped at 25.
    let many: Vec<(UserId, String)> = (1..=40u64)
        .map(|i| (UserId::new(i), format!("u{i}")))
        .collect();
    let v = serde_json::to_value(gp_components(G, 0, 0, &many, true)).unwrap();
    assert_eq!(
        v[0]["components"][0]["options"].as_array().unwrap().len(),
        GP_MAX_PLAYERS
    );
}

#[test]
fn prompt_embeds_json() {
    let opened = GpWindowOpened {
        round_idx: 1,
        total_rounds: 3,
        prompt: "What song do you cry to?".into(),
        closes_at: NOW,
        timer_secs: TIMER,
        generation: 4,
        text_channel: TC,
    };
    let v = serde_json::to_value(gp_prompt_embed(&opened)).unwrap();
    assert_eq!(v["title"], format!("{GP_ROUND_TITLE} 2/3"));
    assert_eq!(v["description"], "**What song do you cry to?**");
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields[0]["name"], GP_PROMPT_HOW_TO_TITLE);
    assert_eq!(fields[1]["name"], GP_PROMPT_CLOSES_TITLE);
    assert!(fields[1]["value"]
        .as_str()
        .unwrap()
        .starts_with(&format!("<t:{NOW}:R>")));

    let closed = GpWindowClosed {
        round_idx: 1,
        total_rounds: 3,
        prompt: "p".into(),
        prompt_message: None,
        count: 4,
        text_channel: TC,
        next: GpNext::Finished(vec![]),
    };
    let v = serde_json::to_value(gp_prompt_closed_embed(&closed)).unwrap();
    let desc = v["description"].as_str().unwrap();
    assert!(
        desc.contains(&format!("{GP_WINDOW_CLOSED} 4 {GP_WINDOW_CLOSED_SONGS}")),
        "{desc}"
    );
    let empty = GpWindowClosed { count: 0, ..closed };
    let v = serde_json::to_value(gp_prompt_closed_embed(&empty)).unwrap();
    assert!(v["description"].as_str().unwrap().contains(GP_WINDOW_EMPTY));

    let w = GpWindowWarning {
        round_idx: 0,
        total_rounds: 1,
        prompt: "p".into(),
        count: 2,
        closes_at: NOW,
        text_channel: TC,
    };
    let text = gp_warning_text(&w);
    assert!(text.starts_with(GP_WINDOW_WARNING));
    assert!(text.contains(&format!("<t:{NOW}:R>")));
}

/// The song message must show the prompt and the track but never a mention.
#[test]
fn track_embed_hides_submitter() {
    let start = GpTrackStart {
        round_idx: 0,
        total_rounds: 2,
        track_idx: 1,
        total_tracks: 3,
        prompt: "Cry song.".into(),
        track: track("secret"),
        players: vec![],
        guessable: true,
        clip: None,
        generation: 1,
        text_channel: TC,
    };
    let v = serde_json::to_value(gp_track_embed(&start)).unwrap();
    assert_eq!(
        v["title"],
        format!("{GP_ROUND_TITLE} 1/2 · {GP_SONG_TITLE} 2/3")
    );
    let desc = v["description"].as_str().unwrap();
    assert!(desc.contains("*Cry song.*"), "{desc}");
    assert!(desc.contains("secret"), "{desc}");
    assert!(desc.contains(GP_ROUND_HINT), "{desc}");
    assert!(desc.contains(GP_LIKE_HINT), "{desc}");
    assert!(!desc.contains("<@"), "must not mention anyone: {desc}");

    let solo = GpTrackStart {
        guessable: false,
        ..start
    };
    let desc = serde_json::to_value(gp_track_embed(&solo)).unwrap()["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!desc.contains(GP_ROUND_HINT), "{desc}");
    assert!(desc.contains(GP_LIKE_HINT), "{desc}");
}

#[test]
fn reveal_embed_json() {
    let res = GpTrackResult {
        round_idx: 0,
        total_rounds: 2,
        track_idx: 0,
        total_tracks: 2,
        prompt: "Cry song.".into(),
        submitter: A,
        title: "song".into(),
        url: "https://example.invalid/song".into(),
        correct: vec![B, C],
        fooled_everyone: false,
        likes: 3,
        played_full: false,
        guessable: true,
        scores: vec![(B, 100), (C, 100), (A, 30)],
        message: None,
        text_channel: TC,
        next: GpNext::Finished(vec![]),
        failed: false,
        held: false,
        round: None,
    };
    let v = serde_json::to_value(gp_reveal_embed(&res)).unwrap();
    assert_eq!(
        v["title"],
        format!("{GP_ROUND_TITLE} 1/2 · {GP_SONG_TITLE} 1/2")
    );
    let desc = v["description"].as_str().unwrap();
    assert!(desc.contains("*Cry song.*"), "{desc}");
    assert!(
        desc.contains("[song](https://example.invalid/song)"),
        "{desc}"
    );
    assert!(desc.contains(&format!("{GP_REVEAL} <@100>")), "{desc}");
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3, "guessed right, likes, scoreboard");
    assert_eq!(fields[0]["name"], GP_GUESSED_RIGHT);
    assert_eq!(fields[0]["value"], "<@200>, <@300>");
    assert_eq!(fields[1]["name"], GP_LIKES);
    assert_eq!(fields[1]["value"], "3");
    assert_eq!(fields[2]["name"], GP_SCOREBOARD);
    assert_eq!(
        fields[2]["value"],
        "1. <@200> — 100\n2. <@300> — 100\n3. <@100> — 30"
    );

    let fooled = GpTrackResult {
        correct: vec![],
        fooled_everyone: true,
        likes: 0,
        ..res.clone()
    };
    let v = serde_json::to_value(gp_reveal_embed(&fooled)).unwrap();
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0]["value"], GP_NOBODY_GUESSED);
    assert_eq!(fields[1]["name"], GP_FOOLED_EVERYONE);
    assert_eq!(fields[1]["value"], "<@100>");

    // Not guessable: no guess fields at all.
    let solo = GpTrackResult {
        correct: vec![],
        guessable: false,
        ..res
    };
    let v = serde_json::to_value(gp_reveal_embed(&solo)).unwrap();
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0]["name"], GP_LIKES);
}

#[test]
fn scoreboard_and_status_embeds_json() {
    let v = serde_json::to_value(gp_scoreboard_embed(&[], GP_GAME_OVER)).unwrap();
    assert_eq!(v["title"], GP_GAME_OVER);
    assert_eq!(v["description"], "-");

    let submitting = GpStatus::Submitting {
        host: A,
        round: 1,
        total: 5,
        prompt: "p".into(),
        closes_at: NOW,
        submitted: vec!["alice".into(), "bob".into()],
        scores: vec![(A, 0)],
    };
    let v = serde_json::to_value(gp_status_embed(&submitting)).unwrap();
    assert_eq!(v["title"], format!("{GP_STATUS_SUBMITTING} 1/5"));
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields[0]["value"], "p");
    assert_eq!(fields[1]["value"], "<@100>");
    assert_eq!(fields[2]["value"], format!("<t:{NOW}:R>"));
    assert_eq!(fields[3]["value"], "alice, bob");
    // Titles never appear in status output.
    assert!(!serde_json::to_string(&v).unwrap().contains("watch?v="));

    let playing = GpStatus::Playing {
        round: 2,
        total: 3,
        track: 1,
        tracks: 4,
        prompt: "p".into(),
        guessed: vec![],
        likes: 2,
        scores: vec![(A, 110)],
    };
    let v = serde_json::to_value(gp_status_embed(&playing)).unwrap();
    assert_eq!(
        v["title"],
        format!("{GP_STATUS_PLAYING} 2/3 · {GP_SONG_TITLE} 1/4")
    );
    let fields = v["fields"].as_array().unwrap();
    assert_eq!(fields[1]["value"], GP_NOBODY_YET);
    assert_eq!(fields[2]["value"], "2");
    assert_eq!(fields[3]["value"], "1. <@100> — 110");
}

/// A round hides the song, and `/nowplaying` names it (the floating status
/// spec, rule 7). The check matches `qualified_name`, so this one entry
/// covers the `np` alias too.
#[test]
fn nowplaying_is_blocked_during_a_game() {
    assert!(GP_BLOCKED_COMMANDS.contains(&"nowplaying"));
}

/// Every blocked name must be a real top-level music command (so the list
/// cannot silently rot), and none of the game's own subcommands may be
/// caught by it. poise fills `qualified_name` only at framework start, so
/// top-level `name`s are what we compare against here.
#[cfg(not(tarpaulin_include))]
#[test]
fn blocklist_matches_registry() {
    let music: Vec<String> = crate::commands::music::music_commands()
        .into_iter()
        .map(|c| c.name.to_string())
        .collect();
    for blocked in GP_BLOCKED_COMMANDS {
        assert!(
            music.contains(&blocked.to_string()),
            "{blocked} is not a registered music command"
        );
    }
    for stalling in GP_STALLING_COMMANDS {
        assert!(
            GP_BLOCKED_COMMANDS.contains(stalling),
            "{stalling} stalls the game and must stay blocked"
        );
    }
    // Moving the bot strands `game.voice_channel`, after which every guess and
    // 👍 is rejected against a channel nobody is in.
    for moving in ["summon", "summonchannel"] {
        assert!(GP_BLOCKED_COMMANDS.contains(&moving), "{moving}");
    }
    // The game has its own `/gp voteskip`; the music one bypasses the majority.
    assert!(GP_BLOCKED_COMMANDS.contains(&"voteskip"));
    assert!(!GP_BLOCKED_COMMANDS.contains(&"gp"));
    // `resume` used to be left off deliberately, as an escape hatch for a
    // queue somebody had paused. The playback lease withdrew that: `/resume`
    // mutates the queue, so it now takes a `QueueGuard` and a guild the game
    // owns refuses it there whatever this list says. Leaving it off only
    // bought a later, less explanatory refusal -- and until the guard
    // existed it bought a real one, `queue.resume()` landing on a live round.
    assert!(GP_BLOCKED_COMMANDS.contains(&"resume"));
    // 🪤 `downvote` (`skip.rs`) belongs to this list by every property it
    // has -- it takes the guard as `Free` and mutates the queue through
    // `force_skip_top_track` -- and is deliberately absent, because it is
    // registered nowhere: `music_commands()` does not list it, so
    // `all_commands()` does not either, and the assertion above would
    // reject it. Registering it is not a formality; as written it
    // `.unwrap()`s `queue().current()` and would panic on an empty queue.
    // The day it is registered, this fails and says what to do about it.
    assert!(
        !music.contains(&"downvote".to_string()),
        "downvote is registered now -- add it to GP_BLOCKED_COMMANDS"
    );
    // A blocked name does nothing unless its command actually runs the
    // check. `remove` sat on this list for exactly that reason with no
    // `check = "cmd_check_music"`, so its entry was inert.
    let by_name: std::collections::HashMap<String, _> = crate::commands::music::music_commands()
        .into_iter()
        .map(|c| (c.name.to_string(), c.checks.len()))
        .collect();
    for blocked in GP_BLOCKED_COMMANDS {
        assert_ne!(
            by_name.get(*blocked).copied().unwrap_or_default(),
            0,
            "{blocked} is blocked but runs no check, so the block never fires"
        );
    }
    for sub in &gp().subcommands {
        let qualified = format!("gp {}", sub.name);
        assert!(!GP_BLOCKED_COMMANDS.contains(&qualified.as_str()));
    }
}

#[cfg(not(tarpaulin_include))]
#[test]
fn command_registration() {
    let all = crate::commands::all_commands();
    let registered = crate::commands::commands_to_register();
    for list in [&all, &registered] {
        assert!(list.iter().any(|c| c.name == "gp"), "gp not registered");
        // `gp` is registered on its own, not by chaining `game_commands()`:
        // coinflip and rolldice are deliberately still unregistered, and
        // pulling them in as a side effect of shipping `gp` is the mistake
        // this guards against.
        for unregistered in ["coinflip", "rolldice"] {
            assert!(
                !list.iter().any(|c| c.name == unregistered),
                "{unregistered} must stay unregistered"
            );
        }
    }

    let cmd = gp();
    assert_eq!(cmd.category.as_deref(), Some("Games"));
    assert!(cmd.guild_only);
    assert!(cmd.aliases.iter().any(|a| a == "guiltypleasure"));
    assert!(cmd.slash_action.is_some() && cmd.prefix_action.is_some());

    let mut names: Vec<&str> = cmd.subcommands.iter().map(|c| c.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["close", "end", "skip", "start", "status", "submit", "votefull", "voteskip"]
    );
    for sub in &cmd.subcommands {
        assert!(sub.guild_only, "{} must be guild_only", sub.name);
        assert!(
            !sub.checks.is_empty(),
            "{} must carry cmd_check_music",
            sub.name
        );
        assert!(
            sub.slash_action.is_some(),
            "{} needs a slash form",
            sub.name
        );
        if sub.name == "submit" {
            assert!(sub.ephemeral, "submit replies must be ephemeral");
            assert!(
                sub.prefix_action.is_none(),
                "submit must be slash-only so the query never lands in the channel"
            );
        } else {
            assert!(
                sub.prefix_action.is_some(),
                "{} should work as a prefix command",
                sub.name
            );
        }
        // A vote is nobody's business but the voter's: a public reply would
        // land under "*name* used `/gp voteskip`" and say exactly who wants
        // the song gone.
        if sub.name == "voteskip" || sub.name == "votefull" {
            assert!(sub.ephemeral, "{} replies must be ephemeral", sub.name);
        }
        if sub.name == "start" {
            let params: Vec<&str> = sub.parameters.iter().map(|p| p.name.as_ref()).collect();
            assert_eq!(
                params,
                vec![
                    "category",
                    "rounds",
                    "timer",
                    "clips",
                    "clip_start",
                    "clip_length",
                    "reveal",
                    "results"
                ]
            );
            assert!(sub.parameters[0].required);
            let reveal = sub.parameters.iter().find(|p| p.name == "reveal").unwrap();
            assert_eq!(reveal.choices.len(), 2, "after each song, or at the end");
            assert_eq!(
                sub.parameters[0].choices.len(),
                crate::commands::music::gp_prompts::GP_PROMPTS.len() + 1,
                "every category + Mixed"
            );
            // Only the category is required; everything else has a default.
            assert!(sub.parameters[1..].iter().all(|p| !p.required));
        }
    }
}

// --- Playback lease ------------------------------------------------
//
// The primitives (`claim_playback`, `release_playback`, `lock_queue`) have
// their own tests in `music/lease.rs`. These exercise the lease as wired
// into the five places that move `gp_games` in this file.

use crate::commands::music::gp_persist::GpPersist;
use tokio::sync::mpsc;

/// A `Data` whose persistence writes land in a channel rather than
/// Postgres. Mirrors `gp_persist::test::recording`.
fn recording() -> (Data, mpsc::UnboundedReceiver<GpPersist>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let data = Data(Arc::new(DataInner {
        gp_persist: Some(tx),
        ..Default::default()
    }));
    (data, rx)
}

/// A minimal game, enough for `gp_restore` to accept.
fn a_game(guild_id: GuildId) -> GpGame {
    GpGame::new(
        guild_id,
        A,
        VC,
        TC,
        GpCategory::Nostalgia,
        prompts(&["p1"]),
        TIMER,
        None,
        GpReveal::default(),
        true,
        NOW,
    )
}

/// Starts a minimal game and asserts it succeeded, so the tests below read
/// as lifecycle rather than setup.
fn start_a_game(data: &Data, guild_id: GuildId) {
    data.gp_start(
        guild_id,
        A,
        "alice".into(),
        VC,
        TC,
        GpCategory::Nostalgia,
        prompts(&["p1"]),
        TIMER,
        None,
        GpReveal::default(),
        true,
        NOW,
    )
    .expect("gp_start should succeed");
}

/// The lease and the games map must agree after EVERY game-lifecycle method.
/// This is the regression guard for the drift the co-location design exists
/// to prevent: a lease that outlives its game wedges /play forever, with no
/// game left to end.
fn assert_lease_agrees(data: &Data, guild_id: GuildId) {
    let has_game = data.gp_games.contains_key(&guild_id);
    let owned = data.playback_owner(guild_id) == PlaybackOwner::Game;
    assert_eq!(
        has_game, owned,
        "gp_games says {has_game} but the lease says {owned} for {guild_id}"
    );
}

#[test]
fn gp_start_claims_playback() {
    let (data, _rx) = recording();
    assert_lease_agrees(&data, G);
    start_a_game(&data, G);
    assert_eq!(data.playback_owner(G), PlaybackOwner::Game);
    assert_lease_agrees(&data, G);
}

#[test]
fn gp_remove_releases_playback() {
    let (data, _rx) = recording();
    start_a_game(&data, G);
    data.gp_remove(G);
    assert_eq!(data.playback_owner(G), PlaybackOwner::Free);
    assert_lease_agrees(&data, G);
}

#[test]
fn gp_remove_if_parked_releases_only_when_it_removes() {
    let (data, _rx) = recording();
    start_a_game(&data, G);

    // Not parked: nothing is removed, so nothing is released.
    assert!(!data.gp_remove_if_parked(G));
    assert_eq!(data.playback_owner(G), PlaybackOwner::Game);
    assert_lease_agrees(&data, G);

    data.gp_park_for_end(G, A, true).expect("park");
    assert!(data.gp_remove_if_parked(G));
    assert_eq!(data.playback_owner(G), PlaybackOwner::Free);
    assert_lease_agrees(&data, G);
}

#[test]
fn gp_restore_claims_playback() {
    let (data, _rx) = recording();
    let game = a_game(G);
    assert!(data.gp_restore(G, game));
    assert_eq!(data.playback_owner(G), PlaybackOwner::Game);
    assert_lease_agrees(&data, G);
}

/// `gp_restore` returns false when `/gp start` got there first. This guards
/// the `Entry::Occupied` arm only: it must never claim, release, or
/// otherwise disturb the lease the running game already holds.
///
/// 🪤 It does NOT exercise the Vacant-arm claim -- it structurally cannot
/// reach that branch. `gp_restore_claims_playback` covers that, and does
/// fail without the claim line.
#[test]
fn restoring_over_a_live_game_leaves_the_lease_alone() {
    let (data, _rx) = recording();
    start_a_game(&data, G);
    assert!(!data.gp_restore(G, a_game(G)));
    assert_lease_agrees(&data, G);
}

/// Everything the subscriber formats lands in one shared buffer the test
/// can read back.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// #468. A refused `/gp` command is answered ephemerally -- correctly -- but
/// used to leave nothing on the server, so "players can't join" had no log
/// line to look at. The refusal must reach the operator as a WARN naming
/// the command, who ran it, where, and why.
#[test]
fn a_refused_gp_command_is_logged_as_well_as_answered() {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_ansi(false)
        .without_time()
        .finish();

    let reply = tracing::subscriber::with_default(subscriber, || {
        refuse_gp(
            "submit",
            UserId::new(424_242_424_242),
            Some(GuildId::new(909_090_909_090)),
            CrackedError::NoGuildId,
        )
    });

    assert!(matches!(
        reply,
        CrackedMessage::CrackedError(CrackedError::NoGuildId)
    ));
    let log = String::from_utf8(captured.0.lock().expect("log buffer").clone())
        .expect("the formatter writes UTF-8");
    assert!(log.contains("WARN"), "not logged at WARN: {log}");
    assert!(log.contains("submit"), "command missing: {log}");
    assert!(log.contains("424242424242"), "user missing: {log}");
    assert!(log.contains("909090909090"), "guild missing: {log}");
    assert!(
        log.contains(&CrackedError::NoGuildId.to_string()),
        "error missing: {log}"
    );
}
