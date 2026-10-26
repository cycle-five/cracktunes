//! The tests of /gp that reach into the glue -- the playback's judgement of a
//! song, the command blocklist against the registry, a refused command -- and
//! the `Data`-shaped helpers the delivery tests share. The game's own rules are
//! tested in `test.rs`, which has no `Data`.
use super::commands::{gp, gp_start, refuse_gp, GpCategoryChoice, GpRevealChoice};
use super::playback::{gp_played, gp_played_at_end};
use super::*;
use crate::commands::music::gp_prompts::GpCategory;
use crate::messaging::message::CrackedMessage;
use crate::{errors::CrackedError, Data, DataInner};
use ::serenity::all::{GuildId, UserId};
use crack_gp::gp::test_support as game_tests;
use crack_gp::gp::GP_STALLING_COMMANDS;
use std::{sync::Arc, time::Duration};

pub(super) use crack_gp::gp::test_support::{A, B, G, NOW, TC};

/// A `Data` with no database; its game handle shares the map and the lease.
pub(super) fn data() -> Data {
    Data(Arc::new(DataInner::default()))
}

pub(super) fn game_with(data: &Data, prompt_list: &[&str]) -> GpWindowOpened {
    game_tests::game_with(&data.gp_data(), prompt_list)
}

pub(super) fn game_with_reveal(
    data: &Data,
    prompt_list: &[&str],
    clip: Option<GpClip>,
    reveal: GpReveal,
) -> GpWindowOpened {
    game_tests::game_with_reveal(&data.gp_data(), prompt_list, clip, reveal)
}

pub(super) fn game_with_settings(
    data: &Data,
    prompt_list: &[&str],
    clip: Option<GpClip>,
    reveal: GpReveal,
    round_results: bool,
) -> GpWindowOpened {
    game_tests::game_with_settings(&data.gp_data(), prompt_list, clip, reveal, round_results)
}

pub(super) fn game_started_at(data: &Data, at: i64) -> GpWindowOpened {
    game_tests::game_started_at(&data.gp_data(), at)
}

pub(super) fn submit(data: &Data, user: UserId, name: &str, title: &str) -> GpSubmitOutcome {
    game_tests::submit(&data.gp_data(), user, name, title)
}

pub(super) fn game(data: &Data) -> GpGame {
    game_tests::game(&data.gp_data())
}

pub(super) fn playing_with_title(data: &Data, title: &str, reveal: GpReveal) -> GpTrackStart {
    game_tests::playing_with_title(&data.gp_data(), title, reveal)
}

fn errored(play_time: Duration) -> songbird::tracks::TrackState {
    use songbird::tracks::{PlayMode, TrackState};
    TrackState {
        playing: PlayMode::Errored(songbird::tracks::PlayError::Create(Arc::new(
            songbird::input::AudioStreamError::Unsupported,
        ))),
        play_time,
        ..Default::default()
    }
}

/// A dead link and a stream that dies part-way through are not the same thing
/// (#423). Only the first reached nobody and pays nothing; the second reached
/// the room, so its guesses and likes stand, but below the played bar nobody
/// could have placed it, so it fools nobody.
#[test]
fn how_much_of_a_song_was_heard() {
    use songbird::tracks::{PlayMode, TrackState};
    use GpPlayed::{CutShort, Heard, Never};
    // Preparing -> Errored without mixing a frame: nobody heard it.
    assert_eq!(gp_played(&errored(Duration::ZERO), None), Never);
    // Died 200ms in, or a second in: the stream never really opened.
    assert_eq!(gp_played(&errored(Duration::from_millis(200)), None), Never);
    assert_eq!(gp_played(&errored(Duration::from_secs(1)), None), Never);
    // Either side of the dead-link line.
    assert_eq!(
        gp_played(&errored(GP_DEAD_LINK - Duration::from_millis(1)), None),
        Never
    );
    assert_eq!(gp_played(&errored(GP_DEAD_LINK), None), CutShort);
    // Died 29 seconds in: the room heard half a minute of it. Before #423 this
    // threw away its guesses and likes and said it could not be played.
    assert_eq!(gp_played(&errored(Duration::from_secs(29)), None), CutShort);
    // Either side of the played line.
    assert_eq!(
        gp_played(&errored(GP_MIN_PLAYED - Duration::from_millis(1)), None),
        CutShort
    );
    assert_eq!(gp_played(&errored(GP_MIN_PLAYED), None), Heard);
    // Died two minutes in: the room heard it, so it scores like any other song.
    assert_eq!(gp_played(&errored(Duration::from_secs(120)), None), Heard);
    // A 45s clip is judged against its own length, not the flat thirty: it
    // needs 22.5s, so 25s counts as heard where a whole song would not.
    let clip45 = Some(Duration::from_secs(45));
    assert_eq!(gp_played(&errored(Duration::from_secs(25)), clip45), Heard);
    assert_eq!(
        gp_played(&errored(Duration::from_secs(20)), clip45),
        CutShort
    );
    // The dead-link line does not scale: a clip that never opened is still dead.
    assert_eq!(gp_played(&errored(Duration::ZERO), clip45), Never);
    // A song that simply finished is heard at any play time.
    let ended = TrackState {
        playing: PlayMode::End,
        play_time: Duration::ZERO,
        ..Default::default()
    };
    assert_eq!(gp_played(&ended, None), Heard);
    assert_eq!(gp_played(&TrackState::default(), None), Heard);
}

/// What the track-end handler decides: a vote for more of the song outranks
/// the play time, and of several states the least heard one stands.
#[test]
fn the_end_handler_weighs_votes_then_the_least_heard_state() {
    use GpPlayed::{CutShort, Heard, Never};
    let dead = errored(Duration::ZERO);
    let short = errored(Duration::from_secs(10));
    let fine = songbird::tracks::TrackState::default();
    assert_eq!(gp_played_at_end(false, [&dead], None), Never);
    assert_eq!(gp_played_at_end(false, [&short], None), CutShort);
    assert_eq!(gp_played_at_end(false, [&fine], None), Heard);
    assert_eq!(gp_played_at_end(false, [&fine, &short], None), CutShort);
    assert_eq!(gp_played_at_end(false, [&short, &dead], None), Never);
    assert_eq!(gp_played_at_end(false, [], None), Heard);
    // Somebody asked for more of it, so it reached the room whatever the
    // stream did afterwards.
    assert_eq!(gp_played_at_end(true, [&dead], None), Heard);
    assert_eq!(gp_played_at_end(true, [&short], None), Heard);
}

/// Every blocked name must be a real top-level music command (so the list
/// cannot silently rot), and none of the game's own subcommands may be
/// caught by it. poise fills `qualified_name` only at framework start, so
/// top-level `name`s are what we compare against here.
#[test]
fn blocklist_matches_registry() {
    let music: Vec<String> = crate::commands::music::music_commands()
        .into_iter()
        .map(|c| c.name.to_string())
        .collect();
    // A qualified name ("shop jump") is a subcommand elsewhere; the economy's
    // registry test checks those carry the music check.
    for blocked in GP_BLOCKED_COMMANDS.iter().filter(|b| !b.contains(' ')) {
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
    // `downvote` skips through the queue like `skip` does, so a game that owns
    // playback must refuse it too.
    assert!(GP_BLOCKED_COMMANDS.contains(&"downvote"));
    // A blocked name does nothing unless its command actually runs the
    // check. `remove` sat on this list for exactly that reason with no
    // `check = "cmd_check_music"`, so its entry was inert.
    let by_name: std::collections::HashMap<String, _> = crate::commands::music::music_commands()
        .into_iter()
        .map(|c| (c.name.to_string(), c.checks.len()))
        .collect();
    for blocked in GP_BLOCKED_COMMANDS.iter().filter(|b| !b.contains(' ')) {
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

#[test]
fn command_registration() {
    let all = crate::commands::all_commands();
    let registered = crate::commands::commands_to_register();
    for list in [&all, &registered] {
        assert!(list.iter().any(|c| c.name == "gp"), "gp not registered");
        // `gp` is registered on its own, not by chaining `game_commands()`:
        // rolldice is deliberately still unregistered, and pulling it in as
        // a side effect of shipping `gp` is the mistake this guards against.
        assert!(
            !list.iter().any(|c| c.name == "rolldice"),
            "rolldice must stay unregistered"
        );
        // The name `coinflip` is now the economy's bet command. The old
        // no-argument game one (`game_commands()`) must stay unregistered.
        let coinflips: Vec<_> = list.iter().filter(|c| c.name == "coinflip").collect();
        assert_eq!(coinflips.len(), 1, "exactly one coinflip registered");
        assert_eq!(coinflips[0].category.as_deref(), Some("Economy"));
        assert!(coinflips[0].parameters.iter().any(|p| p.name == "bet"));
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
            assert!(!sub.parameters[0].required, "no category is Random");
            let reveal = sub.parameters.iter().find(|p| p.name == "reveal").unwrap();
            assert_eq!(reveal.choices.len(), 2, "after each song, or at the end");
            assert_eq!(
                sub.parameters[0].choices.len(),
                crate::commands::music::gp_prompts::GP_PROMPTS.len() + 2,
                "every category + Random + Pick several"
            );
            // Nothing is required; everything has a default.
            assert!(sub.parameters.iter().all(|p| !p.required));
        }
    }
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
        CrackedMessage::CrackedError(ref text) if *text == CrackedError::NoGuildId.to_string()
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

/// What Discord shows and what a prefix command may type, for each variant of
/// a `/gp` choice enum, in dropdown order.
fn pin_choices<T, D>(table: &[(D, &str, &[&str])], to_domain: impl Fn(T) -> D)
where
    T: poise::ChoiceParameter,
    D: PartialEq + std::fmt::Debug + Copy,
{
    let list = T::list();
    assert_eq!(list.len(), table.len());
    for (choice, (variant, shown, aliases)) in list.iter().zip(table) {
        assert_eq!(&choice.name, shown, "{variant:?}: the dropdown name");
        let parsed = |name: &str| T::from_name(name).map(&to_domain);
        assert_eq!(parsed(shown), Some(*variant), "{shown}");
        for alias in *aliases {
            assert_eq!(parsed(alias), Some(*variant), "alias {alias}");
        }
    }
}

/// The dropdown mirrors the domain's categories: a category added to
/// `GpCategory` but not to `GpCategoryChoice` would be missing from `/gp start`.
#[test]
fn the_category_dropdown_lists_every_category_by_its_label() {
    use poise::ChoiceParameter;
    let list = GpCategoryChoice::list();
    // Every category, plus Random and Pick several.
    assert_eq!(list.len(), GpCategory::CATEGORIES.len() + 2);
    for choice in list.iter() {
        let parsed = GpCategoryChoice::from_name(&choice.name).expect(&choice.name);
        assert_eq!(GpCategory::from(parsed).label(), choice.name);
    }
}

/// The names Discord and prefix commands know the `/gp start` choices by,
/// captured at the base (7c7ddd3b) from the domain enums' own derive, and
/// asserted now on the command's mirror enums.
#[test]
fn the_gp_choices_keep_their_names() {
    use crate::commands::music::gp_prompts::GpCategory as C;
    pin_choices::<GpCategoryChoice, GpCategory>(
        &[
            (C::Random, "🎲 Random", &["random", "mixed"]),
            (C::PickSeveral, "☑️ Pick several…", &["pick"]),
            (C::Nostalgia, "🥹 Nostalgia", &["nostalgia"]),
            (C::Dangerous, "🔥 Slightly More Dangerous", &["dangerous"]),
            (
                C::GoodGame,
                "🎶 The Really Good Game Prompts",
                &["good_game"],
            ),
            (C::Car, "🚗 Car / Driving", &["car"]),
            (C::Chill, "🌿 Altered-State / Chill", &["chill"]),
            (C::Emotional, "😭 Emotional", &["emotional"]),
            (C::BadMusic, "🤢 Bad Music", &["bad_music"]),
            (C::HyperSpecific, "🎧 Hyper-Specific", &["hyper_specific"]),
            (C::Revealing, "🖤 Weirdly Revealing", &["revealing"]),
            (C::Chaos, "😂 Game Chaos", &["chaos"]),
            (C::OneWorders, "⚡ One-Worders", &["one_worders"]),
            (C::Social, "🎤 Social / Go-To", &["social"]),
            (C::Guilty, "😈 Guilty Pleasures / Secret Taste", &["guilty"]),
            (C::Romance, "💋 Sex / Romance / Attraction", &["romance"]),
            (C::Damage, "🥀 Emotional Damage", &["damage"]),
            (C::Funny, "🕺 Chaotic / Funny", &["funny"]),
            (C::Personality, "🧠 Personality Reveals", &["personality"]),
        ],
        GpCategory::from,
    );
    pin_choices::<GpRevealChoice, GpReveal>(
        &[
            (GpReveal::Round, "🤐 At the end of the round", &["round"]),
            (GpReveal::Song, "🎉 After each song", &["song"]),
        ],
        GpReveal::from,
    );
}

/// poise takes `/gp start`'s slash description from the fn's doc comment.
#[test]
fn gp_start_keeps_its_description() {
    assert_eq!(
        gp_start().description.as_deref(),
        Some("Start a game in your voice channel: pick a category, rounds, and the submission timer.")
    );
}

/// Parsing is case-insensitive and refuses a name no variant has.
#[test]
fn the_gp_category_choice_parses_as_before() {
    use poise::ChoiceParameter;
    let parse = |name: &str| GpCategoryChoice::from_name(name).map(GpCategory::from);
    for name in ["🎲 Random", "random", "RANDOM", "mixed"] {
        assert_eq!(parse(name), Some(GpCategory::Random), "{name}");
    }
    assert_eq!(parse("pick"), Some(GpCategory::PickSeveral));
    assert_eq!(parse("NOSTALGIA"), Some(GpCategory::Nostalgia));
    assert_eq!(parse("nope"), None);
}
