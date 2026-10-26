//! The delivery of the game's saved form against the messaging fakes: what the
//! results poster, the close-out and the dropdown take-down send and give up on.
use super::test_support::{data, game, game_with, game_with_reveal, rng, submit, B, G, NOW, TC};
use super::*;
use crate::db::GpOutcome;
use crate::gp_persist::{close_out, post_owed_results, take_down_components, GpCloseOut};
use ::serenity::all::MessageId;
use crack_common::messages::{GP_GAME_OVER, GP_SCOREBOARD};
use crack_common::test_support::{FakeTransport, Op};
use crack_common::transport::TransportError;

fn fail_sends(fake: &FakeTransport, n: usize) {
    let mut q = fake.send_failures.lock().unwrap();
    for _ in 0..n {
        q.push_back(TransportError::Other("503".into()));
    }
}

fn embed_title(fake: &FakeTransport, i: usize) -> String {
    let sent = fake.sent.lock().unwrap();
    let e = serde_json::to_value(crack_common::discord::first_embed(&sent[i]).expect("an embed"))
        .unwrap();
    e["title"].as_str().unwrap_or_default().to_string()
}

/// A two-round game whose first round has been revealed in memory but whose
/// results never reached the channel.
fn round_one_owed(data: &GpData) {
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
