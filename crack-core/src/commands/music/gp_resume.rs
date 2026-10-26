//! Resuming a /gp game after a (re)connect: bring back the guild's saved game,
//! rejoin its voice channel and pick the round up where it left off. The game's
//! saved form and its writer are in [`gp_persist`](super::gp_persist).

use super::gp::{
    gp_after_close, gp_play_track, gp_post, gp_spawn_window_timer_secs, now, GpCard, GpGame,
    GpPhase, GpPlayback, GP_RESUME_WINDOW_SECS,
};
use super::gp_persist::{
    close_out, game_id, post_owed_results, take_down_components, vc_members, GpCloseOut,
};
use crate::commands::music_utils::set_global_handlers_with;
use crate::db::{gp_mark_finished, GpOutcome, GpSaved};
use crate::messaging::messages::{
    GP_LOST, GP_RESUMED, GP_RESUMED_SONG, GP_RESUMED_WINDOW, GP_RESUMED_WINDOW_CLOSED,
};
use crate::messaging::transport::{DiscordTransport, Transport};
use crate::Data;
use ::serenity::all::{GenericChannelId, Guild, GuildId};
use poise::serenity_prelude::Context as SerenityContext;
use sqlx::PgPool;
use std::sync::Arc;

/// Give up on a resume entirely: release the lease, mark the game lost in the
/// database, and tell the channel. Shared by every failure path in
/// [`gp_resume_guild`] once the game has been restored to the guild.
///
/// Goes through the method, not a raw map removal: `gp_remove` is the one
/// place a game ends, and it is what releases the playback lease. A raw
/// remove here would leave the guild owned by a game that no longer exists,
/// and `/play` refused forever (`music/lease.rs`).
async fn abandon_resume(
    data: &Data,
    pool: &PgPool,
    guild_id: GuildId,
    started_at: i64,
    text_channel: GenericChannelId,
    transport: &dyn Transport,
) {
    data.gp_remove(guild_id);
    if let Err(e) = gp_mark_finished(pool, game_id(guild_id), started_at, GpOutcome::Lost).await {
        tracing::warn!("gp: marking the game in {guild_id} lost: {e}");
    }
    gp_post(transport, text_channel, GpCard::Line(GP_LOST.into())).await;
}

/// Bring back the guild's live game, if it has one and it is worth bringing
/// back. Called for every guild as it arrives after a (re)connect; a guild with
/// a game already in memory, or with nothing saved, is a quick no.
pub async fn gp_resume_guild(data: &Data, ctx: &SerenityContext, guild: &Guild) {
    let Some(pool) = data.database_pool.as_ref() else {
        return;
    };
    let guild_id = guild.id;
    if data.gp_is_active(guild_id) {
        return;
    }
    let loaded = match GpSaved::load_live(pool, game_id(guild_id)).await {
        Ok(Some(l)) => l,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!("gp: looking for a saved game in {guild_id}: {e}");
            return;
        },
    };
    let started_at = loaded.saved.game.started_at;
    let game = match GpGame::from_saved(&loaded.saved) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(
                "gp: the saved game in {guild_id} could not be loaded ({e}); dropping it"
            );
            if let Err(e) =
                gp_mark_finished(pool, game_id(guild_id), started_at, GpOutcome::Lost).await
            {
                tracing::warn!("gp: marking the unloadable game in {guild_id} lost: {e}");
            }
            return;
        },
    };
    let text_channel = game.text_channel;
    let transport: Arc<dyn Transport> = Arc::new(DiscordTransport::of(ctx));
    let tombstone = |outcome| gp_mark_finished(pool, game_id(guild_id), started_at, outcome);

    if game.phase == GpPhase::Finished {
        tracing::info!("gp: the game in {guild_id} had finished; posting what it still owed");
        close_out(&*transport, &game, GpCloseOut::Finished, tombstone).await;
        return;
    }

    // Alive until when? An open window says so itself: it was open until
    // `closes_at` whatever happened to the bot. A song has no such clock, so the
    // last time the bot wrote or stamped the game stands in.
    let alive_until = match game.phase {
        GpPhase::Submitting => game.rounds[game.current_round].closes_at.unwrap_or(0),
        _ => now() - loaded.last_seen_secs_ago,
    };
    let down_for = now() - alive_until;
    let too_late = down_for > GP_RESUME_WINDOW_SECS;
    let nobody = vc_members(guild, game.voice_channel, ctx.cache.current_user().id) == 0;
    if too_late || nobody {
        let why = if too_late {
            format!("gone for {down_for}s")
        } else {
            "the voice channel is empty".to_string()
        };
        tracing::info!("gp: not resuming the game in {guild_id}: {why}");
        close_out(&*transport, &game, GpCloseOut::Lost, tombstone).await;
        return;
    }

    // A round the game moved past without its results reaching the channel is
    // owed them, and they go up first -- before anything is said about the
    // restart -- so they read as results arriving late.
    let owed = post_owed_results(&*transport, &game).await;

    let (phase, voice_channel, current_round) =
        (game.phase, game.voice_channel, game.current_round);
    let closes_at = game.rounds[current_round].closes_at;
    if !data.gp_restore(guild_id, game) {
        // `/gp start` beat us to it; that game's first snapshot closes this row.
        return;
    }
    // Now that the game is the guild's again, write down that those results are
    // up, or the next restart would post them a second time.
    for idx in owed {
        data.gp_mark_results_posted(guild_id, idx);
    }
    // No invoking user on this path -- the guild is being restored after a
    // restart -- so a refusal is reported to the tracing log and the game's
    // own text channel rather than a reply. It abandons the resume exactly
    // like a failed join does below: otherwise the guild stays owned by a
    // game that can never play again (`music/lease.rs`).
    let permit = match crate::music::perms::ensure_can_join(&ctx.cache, guild_id, voice_channel) {
        Ok(permit) => permit,
        Err(e) => {
            tracing::warn!("gp: cannot rejoin {voice_channel} in {guild_id} to resume: {e}");
            abandon_resume(data, pool, guild_id, started_at, text_channel, &*transport).await;
            return;
        },
    };
    // Through `join_permitted`, not `songbird.join`: this path used to do no
    // cleanup at all after a failed join, so the connectionless `Call` songbird
    // leaves registered survived and the next join found it and skipped the
    // permission gate (#502). There is now one implementation of "join and tidy
    // up if it fails", and this is the same one `do_join` uses.
    let call =
        match crate::commands::music_utils::join_permitted(data, &data.songbird, permit).await {
            Ok(call) => call,
            Err(e) => {
                tracing::warn!("gp: rejoining {voice_channel} in {guild_id} to resume: {e}");
                // Through the method, not the map: `gp_remove` is the one place a
                // game ends, and it is what releases the playback lease. A raw
                // remove here would leave the guild owned by a game that no longer
                // exists, and /play refused forever. Its own bookkeeping write
                // is superseded by `abandon_resume`, which owns the authoritative
                // database write for this path, since `gp_remove`'s in-memory
                // `game` reflects the pre-rejoin-failure state.
                abandon_resume(data, pool, guild_id, started_at, text_channel, &*transport).await;
                return;
            },
        };
    set_global_handlers_with(
        ctx,
        Arc::new(data.clone()),
        call.clone(),
        guild_id,
        text_channel,
    )
    .await;
    let pb = GpPlayback {
        data: Arc::new(data.clone()),
        transport: transport.clone(),
        call,
        guild_id,
    };
    tracing::info!("gp: resuming the game in {guild_id} in {phase:?}, round {current_round}");

    match phase {
        GpPhase::Submitting => {
            let remaining = closes_at.unwrap_or(0) - now();
            let round_no = current_round + 1;
            // The generation the restore left in the map is the one the timer
            // must match; read it back rather than trusting the copy we had.
            let generation = data
                .gp_games
                .get(&guild_id)
                .map(|g| g.generation)
                .unwrap_or(0);
            if remaining <= 0 {
                announce(
                    &pb,
                    text_channel,
                    &GP_RESUMED_WINDOW_CLOSED.replace("{round}", &round_no.to_string()),
                )
                .await;
                // Bound first: a `ThreadRng` temporary is not `Send`, and the
                // condition of an `if let` lives across the body's await.
                let closed = data.gp_close_window_if(guild_id, generation, &mut rand::rng(), now());
                if let Some(closed) = closed {
                    if let Err(e) = gp_after_close(pb, closed).await {
                        tracing::warn!("gp: closing the resumed window in {guild_id}: {e}");
                    }
                }
            } else {
                announce(
                    &pb,
                    text_channel,
                    &format!(
                        "{GP_RESUMED_WINDOW} {round_no}, <t:{}:R>.",
                        closes_at.unwrap_or(0)
                    ),
                )
                .await;
                gp_spawn_window_timer_secs(pb, generation, text_channel, remaining as u64);
            }
        },
        GpPhase::Playing => {
            let Some((start, old_message)) = data.gp_resume_playing(guild_id) else {
                return;
            };
            announce(&pb, text_channel, GP_RESUMED_SONG).await;
            // Restore the one-live-dropdown invariant: the reveal only edits the
            // message the track remembers, which is about to be the new one.
            if let Some((c, m)) = old_message {
                take_down_components(&*pb.transport, guild_id, c, m).await;
            }
            if let Err(e) = gp_play_track(&pb, start).await {
                tracing::warn!("gp: restarting the song in {guild_id}: {e}");
            }
        },
        GpPhase::Finished => unreachable!("finished games are not restored"),
    }
}

async fn announce(pb: &GpPlayback, text_channel: GenericChannelId, what: &str) {
    gp_post(
        &*pb.transport,
        text_channel,
        GpCard::Line(format!("{GP_RESUMED} {what}")),
    )
    .await;
}
