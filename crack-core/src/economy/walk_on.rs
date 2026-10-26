//! Theme playback: the walk-on clip over songbird, the queue and the rooms
//! hub. The glue around `theme.rs`'s pure rules and purchase, kept in
//! crack-core because it reads songbird, the queue and `Data`.

use super::cards::theme_card_text;
use super::target::voice_of;
use super::theme::{
    audible, claim_theme, leave_after_theme, run_clip, still_on, theme_plan, theme_trigger,
    ClipEvent, ClipOutcome, Settled, ThemePlan, THEME_READY_WITHIN,
};
use super::wallet::{ActionPayload, ActionState};
use crate::commands::music_utils::{connected_call, join_permitted, set_global_handlers_with};
use crate::db::economy::{self as db, ActionRow};
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::{courier, render::Rendered, status::DiscordTransport};
use crate::music::audit::{Actor, BotReason};
use crate::Data;
use ::serenity::async_trait;
use chrono::Utc;
use serenity::all::{
    ChannelId, Context as SerenityContext, GenericChannelId, GuildId, UserId, VoiceState,
};
use songbird::tracks::TrackHandle;
use songbird::{Call, Event, EventContext, EventHandler, TrackEvent};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::Instant;
use uuid::Uuid;

/// After a join-play-leave, how long the bot stays before it leaves: a
/// `/play` resolving into the call meanwhile must not have it dropped.
const THEME_LEAVE_GRACE: Duration = Duration::from_secs(30);

/// How long a theme track stays marked after its player is done. Its end
/// reaches the global track-end handler within a tick of the stop; this only
/// bounds the set when no end ever comes (the bot left first).
const THEME_TRACK_FORGET_AFTER: Duration = Duration::from_secs(60);

/// A voice change: play the theme waiting for this member if it is a join
/// (or the move out of the rooms hub, [`theme_trigger`]). Never waits: the
/// work is spawned.
pub fn on_voice_state(
    data: &Data,
    ctx: &SerenityContext,
    old: Option<&VoiceState>,
    new: &VoiceState,
) {
    let Some(guild) = new.guild_id else {
        return;
    };
    // Off (the default) costs nothing more than this.
    if data.database_pool.is_none() || !data.economy.is_enabled(guild) {
        return;
    }
    let rooms = data.rooms.settings(guild);
    let hub = rooms.hub.filter(|_| rooms.enabled);
    if let Some(channel) = theme_trigger(old.and_then(|o| o.channel_id), new.channel_id, hub) {
        on_voice_join(data.clone(), ctx.clone(), guild, new.user_id, channel);
    }
}

/// A member arrived in `channel`: play the theme waiting for them, off the
/// event path. Nothing happens for the bot itself, or in a server with no
/// database or the economy off; the row waits, and is refunded at its end
/// if it never plays.
pub fn on_voice_join(
    data: Data,
    ctx: SerenityContext,
    guild: GuildId,
    user: UserId,
    channel: ChannelId,
) {
    if data.database_pool.is_none() || !data.economy.is_enabled(guild) {
        return;
    }
    if user == ctx.cache.current_user().id {
        return;
    }
    super::effects::spawn_one_shot("theme", async move {
        play_on_join(&data, &ctx, guild, user, channel).await;
    });
}

/// The channel the bot is connected to through `call`, if any.
async fn channel_of(call: Option<&Arc<Mutex<Call>>>) -> Option<ChannelId> {
    let call = call?;
    let channel = call.lock().await.current_channel();
    channel.map(|c| ChannelId::new(c.get()))
}

/// Claim, plan, play and settle; then, after a join-play-leave, leave if
/// the bot came only for this.
async fn play_on_join(
    data: &Data,
    ctx: &SerenityContext,
    guild: GuildId,
    user: UserId,
    channel: ChannelId,
) {
    let Some(row) = claim_theme(&data.economy_data(), guild, user, Utc::now()).await else {
        return;
    };
    // The reads took a moment: they may have left or moved since.
    if voice_of(&ctx.cache, guild, user) != Some(channel) {
        return;
    }
    let turn = data.economy.try_theme_guard(guild);
    let call = connected_call(&data.songbird, guild, None).await;
    let bot_in = channel_of(call.as_ref()).await;
    let plan = theme_plan(bot_in, channel, data.gp_is_active(guild), turn.is_none());
    let (call, plan) = match (plan, call) {
        (ThemePlan::MixHere, Some(call)) => (call, ThemePlan::MixHere),
        (ThemePlan::JoinPlayLeave, _) => match join_for_theme(data, ctx, guild, channel).await {
            Some(joined) => joined,
            None => return,
        },
        _ => {
            tracing::debug!("economy: theme {} in {guild} waits ({plan:?})", row.id);
            return;
        },
    };
    let track = play(data, ctx, guild, &call, &row, user, channel).await;
    // The clip is over: the next theme in this server may play.
    drop(turn);
    if plan == ThemePlan::JoinPlayLeave {
        tokio::time::sleep(THEME_LEAVE_GRACE).await;
        leave_if_idle(data, guild, &call, channel).await;
    }
    if let Some(track) = track {
        tokio::time::sleep(THEME_TRACK_FORGET_AFTER).await;
        data.economy.take_theme_track(track);
    }
}

/// Join `channel` for a theme, as the queue resume does. Just before the
/// join the plan is made again: a `/play` may have connected the bot since.
/// Connected in `channel` now, the theme mixes there (and the bot stays);
/// anywhere else, or a game started, or a join refused or failed, and the
/// theme waits for a later join.
async fn join_for_theme(
    data: &Data,
    ctx: &SerenityContext,
    guild: GuildId,
    channel: ChannelId,
) -> Option<(Arc<Mutex<Call>>, ThemePlan)> {
    let permit = match crate::music::perms::ensure_can_join(&ctx.cache, guild, channel) {
        Ok(permit) => permit,
        Err(err) => {
            tracing::info!("economy: no theme in {channel} ({guild}): can't join: {err}");
            return None;
        },
    };
    let now = connected_call(&data.songbird, guild, None).await;
    let bot_in = channel_of(now.as_ref()).await;
    match (
        theme_plan(bot_in, channel, data.gp_is_active(guild), false),
        now,
    ) {
        (ThemePlan::JoinPlayLeave, _) => {},
        (ThemePlan::MixHere, Some(call)) => return Some((call, ThemePlan::MixHere)),
        (plan, _) => {
            tracing::debug!("economy: no theme join in {guild}: the bot moved ({plan:?})");
            return None;
        },
    }
    let call = match join_permitted(data, &data.songbird, permit).await {
        Ok(call) => call,
        Err(err) => {
            tracing::warn!("economy: joining {channel} in {guild} for a theme failed: {err:?}");
            return None;
        },
    };
    set_global_handlers_with(
        ctx,
        Arc::new(data.clone()),
        call.clone(),
        guild,
        GenericChannelId::new(channel.get()),
    )
    .await;
    Some((call, ThemePlan::JoinPlayLeave))
}

/// Play `row`'s clip over `call`'s queue and settle it: spent and carded
/// once audible, refunded if it never is. Returns once the clip has stopped,
/// with the track's id when one was started.
async fn play(
    data: &Data,
    ctx: &SerenityContext,
    guild: GuildId,
    call: &Arc<Mutex<Call>>,
    row: &ActionRow,
    target: UserId,
    channel: ChannelId,
) -> Option<Uuid> {
    let pool = data.database_pool.as_ref()?;
    let unplayable = |why: &str| {
        tracing::warn!("economy: theme {} in {guild} could not play: {why}", row.id);
    };
    // Claimed a moment ago, but a cleanse, a cancel or its end may have
    // refunded it since, and the target may have left or moved (a join
    // takes seconds).
    let state = match db::action_state(pool, row.id).await {
        Ok(state) => state,
        Err(err) => {
            tracing::warn!("economy: reading theme {} failed: {err}", row.id);
            return None;
        },
    };
    if !still_on(state, voice_of(&ctx.cache, guild, target), channel) {
        tracing::debug!("economy: theme {} waits ({state:?}, target moved?)", row.id);
        return None;
    }
    let ActionPayload::Theme { url, start_secs } = &row.payload else {
        unplayable("its payload is not a theme's");
        refund_unplayable(pool, row.id).await;
        return None;
    };
    // The link's only way to yt-dlp.
    let input = match crack_types::ytdl_for_url(crate::http_utils::get_client_old().clone(), url) {
        Ok(ytdl) => ytdl.into(),
        Err(err) => {
            unplayable(&format!("its link was refused: {err}"));
            refund_unplayable(pool, row.id).await;
            return None;
        },
    };
    // The guild's music volume, as the queue's tracks play at.
    let (volume, _) = data.get_volume(guild).await;
    // Marked before the track exists, so its end can never reach the
    // queue's handler unmarked.
    let id = Uuid::new_v4();
    data.economy.add_theme_track(id);
    let handle = {
        let mut call = call.lock().await;
        crate::music::queue::play_beside_queue(
            &mut call,
            crate::music::queue::new_track(input, None, Some(row.buyer)),
            id,
            volume,
        )
    };
    // Armed before anything is awaited: the seek below is what opens the
    // stream, and so what fires `Playable` (as `/gp`'s clip does).
    let (tx, mut events) = mpsc::unbounded_channel();
    for (event, signal) in [
        (TrackEvent::Playable, ClipEvent::Playable),
        (TrackEvent::Error, ClipEvent::Errored),
        (TrackEvent::End, ClipEvent::Ended),
    ] {
        let armed = crate::handlers::add_track_handler(
            &handle,
            Event::Track(event),
            ClipSignal {
                tx: tx.clone(),
                event: signal,
            },
        );
        if let Err(err) = armed {
            unplayable(&format!("arming its {event:?} handler failed: {err}"));
            stop_theme(&handle, guild);
            refund_unplayable(pool, row.id).await;
            return Some(id);
        }
    }
    // Only the handlers hold a sender now: when the track goes, so do they.
    drop(tx);
    let seek = (*start_secs > 0).then(|| {
        handle
            .seek(Duration::from_secs(u64::from(*start_secs)))
            .result_async()
    });
    if let Err(why) = audible(seek, &mut events, THEME_READY_WITHIN).await {
        unplayable(&format!("{why:?}"));
        stop_theme(&handle, guild);
        refund_unplayable(pool, row.id).await;
        return Some(id);
    }
    // Audible: the 10 s run from here, whatever the settle and the card do.
    let audible_at = Instant::now();
    let settle = async {
        match db::finish_action(pool, row.id, ActionState::Done).await {
            Ok(true) => {
                // Spawned: a slow post never holds anything up.
                let (ctx, row) = (ctx.clone(), row.clone());
                super::effects::spawn_one_shot("theme card", async move {
                    post_card(&ctx, guild, &row, target, channel).await;
                });
                Settled::Spent
            },
            Ok(false) => Settled::Gone,
            Err(err) => {
                tracing::warn!("economy: marking theme {} played failed: {err}", row.id);
                Settled::Unknown
            },
        }
    };
    match run_clip(&mut events, audible_at, settle, || {
        stop_theme(&handle, guild)
    })
    .await
    {
        ClipOutcome::EndedAtOnce => {
            unplayable("it ended as it started");
            refund_unplayable(pool, row.id).await;
        },
        ClipOutcome::Refunded => {
            tracing::debug!(
                "economy: theme {} was refunded as it played; stopped",
                row.id
            );
        },
        ClipOutcome::Played { .. } => {},
    }
    Some(id)
}

/// Give the buyer their credits back: the theme never played.
async fn refund_unplayable(pool: &sqlx::PgPool, id: i64) {
    if let Err(err) = db::refund_active(pool, id, "unplayable").await {
        tracing::warn!("economy: refunding unplayable theme {id} failed: {err}");
    }
}

/// The theme card, in the chat of the target's voice channel. A failed post
/// is logged: there is no interaction to answer instead.
async fn post_card(
    ctx: &SerenityContext,
    guild: GuildId,
    row: &ActionRow,
    target: UserId,
    channel: ChannelId,
) {
    let text = theme_card_text(target, row.note.as_deref(), row.signed.then_some(row.buyer));
    if let Err(err) = courier::post_message(
        &DiscordTransport::of(ctx),
        GenericChannelId::new(channel.get()),
        &Rendered::text(text),
    )
    .await
    {
        tracing::warn!("economy: posting the theme card in {channel} ({guild}) failed: {err:?}");
    }
}

/// Leave after a join-play-leave's grace, unless music started meanwhile,
/// the bot moved, or another theme is playing. The theme guard is held
/// while leaving, so no theme starts into a call being dropped.
async fn leave_if_idle(data: &Data, guild: GuildId, call: &Arc<Mutex<Call>>, joined: ChannelId) {
    let turn = data.economy.try_theme_guard(guild);
    let (empty, here) = {
        let call = call.lock().await;
        (
            call.queue().is_empty(),
            call.current_channel().map(|c| ChannelId::new(c.get())),
        )
    };
    if !leave_after_theme(
        empty,
        here,
        joined,
        turn.is_none(),
        data.gp_is_active(guild),
    ) {
        return;
    }
    if let Err(err) = crate::music::disconnect::disconnect(
        data,
        &data.songbird,
        guild,
        Actor::bot(BotReason::Theme),
    )
    .await
    {
        tracing::warn!("economy: leaving {joined} in {guild} after a theme failed: {err:?}");
    }
}

/// Stop a theme's track. It may have gone already (ended, or removed by a
/// failed seek): that is no error.
#[expect(
    clippy::disallowed_methods,
    reason = "a walk-on theme ending its own clip: it is mixed over the queue, not in it, so this is no skip"
)]
fn stop_theme(handle: &TrackHandle, guild: GuildId) {
    if let Err(err) = handle.stop() {
        tracing::debug!("economy: the theme in {guild} had already stopped: {err}");
    }
}

/// Passes one of a theme track's events to the task playing it.
struct ClipSignal {
    tx: mpsc::UnboundedSender<ClipEvent>,
    event: ClipEvent,
}

#[async_trait]
impl EventHandler for ClipSignal {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        // The player may have given up and gone: nobody left to tell.
        let _ = self.tx.send(self.event);
        None
    }
}
