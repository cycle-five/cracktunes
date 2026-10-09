//! /gp's Discord glue against the messaging fakes: what each step of a game
//! sends, edits and gives up on. The game's rules are tested in `test.rs`.
use super::playback::{gp_after_close, gp_answer_component, gp_spawn_window_timer_secs};
use super::test::{data, game, game_with, game_with_reveal, rng, submit, A, B, G, NOW, TC};
use super::*;
use crate::messaging::messages::{GP_ABORTED, GP_GAME_OVER, GP_WINDOW_WARNING};
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
    gp_advance_track(playback(&data, &fake), 0, 0, false)
        .await
        .unwrap();
    assert_eq!(fake.ops()[0], Op::Edit(TC.get(), 88));
    let reveal = fake.sent.lock().unwrap()[0].clone();
    assert!(reveal.embed.is_some() && reveal.components.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_reveal_whose_edit_fails_is_posted_instead() {
    let data = data();
    one_song_playing(&data, &["only"]);
    let fake = Arc::new(FakeTransport::default());
    *fake.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
    gp_advance_track(playback(&data, &fake), 0, 0, false)
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
    gp_advance_track(playback(&data, &fake), 0, 0, false)
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
    gp_advance_track(playback(&data, &fake), 0, 0, false)
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
    gp_advance_track(playback(&data, &fake), 0, 0, false)
        .await
        .unwrap();
    let last = fake.ops().len() - 1;
    assert_eq!(embed_title(&fake, last), GP_GAME_OVER);
    assert!(!data.gp_is_active(G), "a finished game is removed");
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

use crate::commands::music::gp_persist::{post_owed_results, take_down_components};

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
