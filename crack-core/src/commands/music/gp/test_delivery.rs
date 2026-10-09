//! /gp's Discord glue against the messaging fakes: what each step of a game
//! sends, edits and gives up on. The game's rules are tested in `test.rs`.
use super::commands::gp_spawn_park_backstop;
use super::playback::{gp_after_close, gp_answer_component, gp_spawn_window_timer_secs};
use super::test::{
    data, game, game_started_at, game_with, game_with_reveal, game_with_settings,
    playing_with_title, rng, submit, A, B, G, NOW, TC,
};
use super::*;
use crate::commands::music::gp_persist::{
    close_out, post_owed_results, take_down_components, GpCloseOut,
};
use crate::db::GpOutcome;
use crate::messaging::messages::{
    GP_ABORTED, GP_ALREADY_SAVED, GP_GAME_OVER, GP_SAVED, GP_SAVED_TO, GP_SCOREBOARD,
    GP_WINDOW_WARNING,
};
use crate::messaging::test_support::{FakePress, FakeTransport, Op, PressOp};
use crate::messaging::transport::TransportError;
use crate::music::ops::test_support::standalone_call;
use crate::Data;
use ::serenity::all::MessageId;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

fn playback(data: &Data, fake: &Arc<FakeTransport>) -> GpPlayback {
    GpPlayback {
        data: Arc::new(data.clone()),
        transport: fake.clone(),
        call: Arc::new(Mutex::new(standalone_call(G, A))),
        guild_id: G,
    }
}

fn fail_sends(fake: &FakeTransport, n: usize) {
    let mut q = fake.send_failures.lock().unwrap();
    for _ in 0..n {
        q.push_back(TransportError::Other("503".into()));
    }
}

fn embed_title(fake: &FakeTransport, i: usize) -> String {
    let sent = fake.sent.lock().unwrap();
    let e = serde_json::to_value(sent[i].embed.clone().expect("an embed")).unwrap();
    e["title"].as_str().unwrap_or_default().to_string()
}

#[tokio::test]
async fn a_round_opens_by_posting_its_prompt_and_recording_where() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    let fake = Arc::new(FakeTransport::default());
    gp_open_round(&playback(&data, &fake), opened)
        .await
        .unwrap();
    assert_eq!(fake.ops(), vec![Op::Send(TC.get())]);
    assert!(fake.sent.lock().unwrap()[0].components.is_empty());
    assert_eq!(
        game(&data).rounds[0].prompt_message,
        Some((TC, MessageId::new(1000)))
    );
}

#[tokio::test]
async fn a_prompt_that_fails_once_is_sent_again() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    let fake = Arc::new(FakeTransport::default());
    fail_sends(&fake, 1);
    gp_open_round(&playback(&data, &fake), opened)
        .await
        .unwrap();
    assert_eq!(fake.ops(), vec![Op::Send(TC.get()), Op::Send(TC.get())]);
    assert!(data.gp_is_active(G));
    assert_eq!(
        game(&data).rounds[0].prompt_message,
        Some((TC, MessageId::new(1000)))
    );
}

#[tokio::test]
async fn a_prompt_that_fails_twice_ends_the_game_and_says_so() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    let fake = Arc::new(FakeTransport::default());
    fail_sends(&fake, 2);
    gp_open_round(&playback(&data, &fake), opened)
        .await
        .unwrap();
    assert!(!data.gp_is_active(G), "the game is discarded");
    assert_eq!(fake.ops().len(), 3);
    assert_eq!(fake.texts().last().unwrap(), GP_ABORTED);
    assert!(
        fake.sent.lock().unwrap()[2].embed.is_none(),
        "the abort is a line"
    );
}

#[tokio::test]
async fn the_close_edits_the_prompt_in_place() {
    let data = data();
    let opened = game_with(&data, &["first", "second"]);
    data.gp_set_prompt_message(G, 0, TC, MessageId::new(77))
        .unwrap();
    let closed = data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    let fake = Arc::new(FakeTransport::default());
    gp_after_close(playback(&data, &fake), closed)
        .await
        .unwrap();
    // Nobody submitted, so the next round opens straight away.
    assert_eq!(fake.ops(), vec![Op::Edit(TC.get(), 77), Op::Send(TC.get())]);
}

#[tokio::test]
async fn a_close_whose_edit_fails_is_posted_instead() {
    let data = data();
    let opened = game_with(&data, &["first", "second"]);
    data.gp_set_prompt_message(G, 0, TC, MessageId::new(77))
        .unwrap();
    let closed = data
        .gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    let fake = Arc::new(FakeTransport::default());
    *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
    gp_after_close(playback(&data, &fake), closed)
        .await
        .unwrap();
    assert_eq!(
        fake.ops(),
        vec![
            Op::Edit(TC.get(), 77),
            Op::Send(TC.get()),
            Op::Send(TC.get())
        ]
    );
}

/// A game of `rounds` prompts, bob's one song submitted, the window closed,
/// and the song message recorded as message 88.
fn one_song_playing(data: &Data, rounds: &[&str]) {
    let opened = game_with_reveal(data, rounds, None, GpReveal::Round);
    submit(data, B, "bob", "Song");
    data.gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    data.gp_set_track_message(G, 0, 0, TC, MessageId::new(88))
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn the_reveal_replaces_the_song_message_and_its_controls() {
    let data = data();
    one_song_playing(&data, &["only"]);
    let fake = Arc::new(FakeTransport::default());
    gp_advance_track(playback(&data, &fake), 0, 0, GpPlayed::Heard)
        .await
        .unwrap();
    assert_eq!(fake.ops()[0], Op::Edit(TC.get(), 88));
    let reveal = fake.sent.lock().unwrap()[0].clone();
    assert!(reveal.embed.is_some() && reveal.components.is_empty());
}

/// However the song ended, the advance takes that path and the song keeps it:
/// the round's results derive the payout from it again (#423).
#[tokio::test(start_paused = true)]
async fn the_advance_keeps_how_much_of_the_song_was_heard() {
    for played in [GpPlayed::Heard, GpPlayed::CutShort, GpPlayed::Never] {
        let data = data();
        one_song_playing(&data, &["first", "second"]);
        let fake = Arc::new(FakeTransport::default());
        gp_advance_track(playback(&data, &fake), 0, 0, played)
            .await
            .unwrap();
        assert_eq!(game(&data).rounds[0].tracks[0].played, played);
    }
}

#[tokio::test(start_paused = true)]
async fn a_reveal_whose_edit_fails_is_posted_instead() {
    let data = data();
    one_song_playing(&data, &["only"]);
    let fake = Arc::new(FakeTransport::default());
    *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
    gp_advance_track(playback(&data, &fake), 0, 0, GpPlayed::Heard)
        .await
        .unwrap();
    assert_eq!(
        fake.ops()[..2],
        [Op::Edit(TC.get(), 88), Op::Send(TC.get())]
    );
}

#[tokio::test(start_paused = true)]
async fn the_rounds_last_reveal_posts_its_results_and_marks_them() {
    let data = data();
    one_song_playing(&data, &["first", "second"]);
    let fake = Arc::new(FakeTransport::default());
    gp_advance_track(playback(&data, &fake), 0, 0, GpPlayed::Heard)
        .await
        .unwrap();
    // The reveal edit, the results, then round two's prompt.
    assert_eq!(
        fake.ops(),
        vec![
            Op::Edit(TC.get(), 88),
            Op::Send(TC.get()),
            Op::Send(TC.get())
        ]
    );
    assert!(game(&data).rounds[0].results_posted);
}

#[tokio::test(start_paused = true)]
async fn a_failed_results_post_leaves_the_round_owed_and_the_game_moves_on() {
    let data = data();
    one_song_playing(&data, &["first", "second"]);
    let fake = Arc::new(FakeTransport::default());
    fail_sends(&fake, 1);
    gp_advance_track(playback(&data, &fake), 0, 0, GpPlayed::Heard)
        .await
        .unwrap();
    assert!(
        !game(&data).rounds[0].results_posted,
        "the next resume posts it"
    );
    assert_eq!(fake.ops().len(), 3, "round two still opens");
    assert_eq!(game(&data).current_round, 1);
}

#[tokio::test(start_paused = true)]
async fn the_games_last_reveal_posts_the_final_scoreboard() {
    let data = data();
    one_song_playing(&data, &["only"]);
    let fake = Arc::new(FakeTransport::default());
    gp_advance_track(playback(&data, &fake), 0, 0, GpPlayed::Heard)
        .await
        .unwrap();
    let last = fake.ops().len() - 1;
    assert_eq!(embed_title(&fake, last), GP_GAME_OVER);
    assert!(!data.gp_is_active(G), "a finished game is removed");
}

#[tokio::test(start_paused = true)]
async fn a_final_scoreboard_that_cannot_be_posted_is_an_error_and_the_game_is_gone() {
    let data = data();
    let opened = game_with_settings(&data, &["only"], None, GpReveal::Song, false);
    submit(&data, B, "bob", "Song");
    data.gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    data.gp_set_track_message(G, 0, 0, TC, MessageId::new(88))
        .unwrap();
    let fake = Arc::new(FakeTransport::default());
    // The reveal is an edit, so the first send is the scoreboard.
    fail_sends(&fake, 1);
    let out = gp_advance_track(playback(&data, &fake), 0, 0, GpPlayed::Heard).await;
    assert!(out.is_err(), "the scoreboard's `?` propagates");
    assert!(!data.gp_is_active(G), "the game is removed before the post");
}

#[tokio::test]
async fn a_song_that_fails_to_post_twice_aborts_the_game() {
    let data = data();
    let start = playing_with_title(&data, "Song", GpReveal::Song);
    let fake = Arc::new(FakeTransport::default());
    fail_sends(&fake, 2);
    gp_play_track(&playback(&data, &fake), start).await.unwrap();
    assert_eq!(
        fake.ops(),
        vec![Op::Send(TC.get()), Op::Send(TC.get()), Op::Send(TC.get())]
    );
    assert_eq!(fake.texts().last().unwrap(), GP_ABORTED);
    assert!(!data.gp_is_active(G));
}

#[tokio::test(start_paused = true)]
async fn the_window_warning_is_a_line() {
    let data = data();
    let opened = game_with(&data, &["first"]);
    submit(&data, B, "bob", "Song");
    let fake = Arc::new(FakeTransport::default());
    gp_spawn_window_timer_secs(playback(&data, &fake), opened.generation, TC, 40);
    tokio::time::sleep(Duration::from_secs(11)).await;
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "the warning, and the close not yet");
    assert!(sent[0].embed.is_none());
    assert!(sent[0]
        .content
        .as_deref()
        .unwrap()
        .starts_with(GP_WINDOW_WARNING));
}

#[tokio::test]
async fn a_click_is_answered_once_privately_and_never_acknowledged() {
    let press = FakePress::default();
    gp_answer_component(&press, "noted".into()).await.unwrap();
    assert_eq!(
        press.ops(),
        vec![PressOp::Respond {
            ephemeral: true,
            text: "noted".into()
        }]
    );
}

/// Save uses the same one-response path as a guess or a 👍: the arm works out
/// the line, then `gp_answer_component` sends it. No acknowledge first.
#[tokio::test]
async fn a_save_line_is_one_private_response() {
    for text in [
        format!("{GP_SAVED} **Full Song** {GP_SAVED_TO}"),
        format!("{GP_ALREADY_SAVED} **Full Song**."),
    ] {
        let press = FakePress::default();
        gp_answer_component(&press, text.clone()).await.unwrap();
        assert_eq!(
            press.ops(),
            vec![PressOp::Respond {
                ephemeral: true,
                text,
            }]
        );
    }
}

/// A two-round game whose first round has been revealed in memory but whose
/// results never reached the channel.
fn round_one_owed(data: &Data) {
    let opened = game_with_reveal(data, &["first", "second"], None, GpReveal::Round);
    submit(data, B, "bob", "Song");
    data.gp_close_window_if(G, opened.generation, &mut rng(), NOW)
        .unwrap();
    data.gp_reveal_and_advance(G, 0, 0, NOW).unwrap();
}

#[tokio::test]
async fn owed_results_are_posted_and_reported() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    assert_eq!(post_owed_results(&fake, &game(&data)).await, vec![0]);
    assert_eq!(fake.ops(), vec![Op::Send(TC.get())]);
}

#[tokio::test]
async fn owed_results_that_fail_are_not_reported_posted() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    fail_sends(&fake, 1);
    assert!(post_owed_results(&fake, &game(&data)).await.is_empty());
    assert_eq!(fake.ops(), vec![Op::Send(TC.get())], "a send was attempted");
}

/// The backstop `/gp end` leaves behind collects the game it was spawned for
/// if that game's `End` never came, and nothing else (#423).
#[tokio::test(start_paused = true)]
async fn the_end_backstop_collects_its_own_game_and_only_that() {
    let data = data();
    game_with(&data, &["first"]);
    let first = data.gp_park_for_end(G, A, false).unwrap();
    gp_spawn_park_backstop(data.clone(), G, first.started_at);
    tokio::time::sleep(Duration::from_secs(GP_PARK_GRACE_SECS + 1)).await;
    assert!(
        !data.gp_is_active(G),
        "its End never came, so it is collected"
    );

    // A game parked after another `/gp end`'s backstop was spawned.
    let data = super::test::data();
    game_with(&data, &["first"]);
    let first = data.gp_park_for_end(G, A, false).unwrap();
    gp_spawn_park_backstop(data.clone(), G, first.started_at);
    assert!(data.gp_remove_if_parked(G), "its End collects it");
    game_started_at(&data, NOW + 5);
    data.gp_park_for_end(G, A, false).unwrap();
    tokio::time::sleep(Duration::from_secs(GP_PARK_GRACE_SECS + 1)).await;
    assert!(data.gp_is_active(G), "not the game it was spawned for");
}

// A game the resume will not bring back is closed out by its tombstone, and
// says so only once that has landed: `on_guild_create` runs on every
// reconnect, and until the tombstone is written the row is still live (#469).

async fn store_down(_: GpOutcome) -> sqlx::Result<bool> {
    Err(sqlx::Error::PoolTimedOut)
}

async fn store_up(_: GpOutcome) -> sqlx::Result<bool> {
    Ok(true)
}

#[tokio::test]
async fn reconnects_while_writes_fail_post_the_owed_results_once() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    // Two reconnects with the store refusing writes, then one with it back.
    // After that the row is finished and the next reconnect never loads it.
    close_out(&fake, &game(&data), GpCloseOut::Lost, store_down).await;
    close_out(&fake, &game(&data), GpCloseOut::Lost, store_down).await;
    close_out(&fake, &game(&data), GpCloseOut::Lost, store_up).await;
    assert_eq!(fake.ops(), vec![Op::Send(TC.get()), Op::Send(TC.get())]);
}

#[tokio::test]
async fn a_lost_game_whose_tombstone_fails_posts_nothing() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    close_out(&fake, &game(&data), GpCloseOut::Lost, store_down).await;
    assert_eq!(fake.ops(), vec![]);
}

#[tokio::test]
async fn a_finished_game_whose_tombstone_fails_posts_nothing() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    close_out(&fake, &game(&data), GpCloseOut::Finished, store_down).await;
    assert_eq!(fake.ops(), vec![]);
}

#[tokio::test]
async fn a_row_something_else_closed_first_posts_nothing() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    close_out(&fake, &game(&data), GpCloseOut::Lost, |_| async {
        Ok(false)
    })
    .await;
    assert_eq!(fake.ops(), vec![], "whoever closed it says so");
}

#[tokio::test]
async fn the_tombstone_is_written_with_the_ending_it_closes_out() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    let seen = std::sync::Mutex::new(Vec::new());
    for ending in [GpCloseOut::Lost, GpCloseOut::Finished] {
        close_out(&fake, &game(&data), ending, |o| {
            seen.lock().unwrap().push(o);
            async { Err(sqlx::Error::PoolTimedOut) }
        })
        .await;
    }
    assert_eq!(
        *seen.lock().unwrap(),
        vec![GpOutcome::Lost, GpOutcome::Finished]
    );
}

#[tokio::test]
async fn a_lost_game_posts_its_owed_results_then_the_lost_scoreboard() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    close_out(&fake, &game(&data), GpCloseOut::Lost, store_up).await;
    assert_eq!(fake.ops(), vec![Op::Send(TC.get()), Op::Send(TC.get())]);
    assert_eq!(embed_title(&fake, 1), GP_SCOREBOARD);
}

#[tokio::test]
async fn a_lost_game_that_owed_no_results_still_posts_its_scoreboard() {
    let data = data();
    game_with(&data, &["first"]);
    let fake = FakeTransport::default();
    close_out(&fake, &game(&data), GpCloseOut::Lost, store_up).await;
    assert_eq!(fake.ops(), vec![Op::Send(TC.get())]);
    assert_eq!(embed_title(&fake, 0), GP_SCOREBOARD);
}

#[tokio::test]
async fn a_finished_game_posts_its_owed_results_then_the_final_scoreboard() {
    let data = data();
    round_one_owed(&data);
    let fake = FakeTransport::default();
    close_out(&fake, &game(&data), GpCloseOut::Finished, store_up).await;
    assert_eq!(fake.ops(), vec![Op::Send(TC.get()), Op::Send(TC.get())]);
    assert_eq!(embed_title(&fake, 1), GP_GAME_OVER);
}

#[tokio::test]
async fn a_finished_game_that_owed_nothing_posts_nothing() {
    let data = data();
    game_with(&data, &["first"]);
    let fake = FakeTransport::default();
    close_out(&fake, &game(&data), GpCloseOut::Finished, store_up).await;
    assert_eq!(fake.ops(), vec![], "its scoreboard may well be up already");
}

#[tokio::test]
async fn taking_down_the_old_dropdown_clears_components_and_nothing_else() {
    let fake = FakeTransport::default();
    take_down_components(&fake, G, TC, MessageId::new(88)).await;
    assert_eq!(fake.ops(), vec![Op::ClearComponents(TC.get(), 88)]);
    assert!(
        fake.sent.lock().unwrap().is_empty(),
        "the embed is left alone"
    );
}
