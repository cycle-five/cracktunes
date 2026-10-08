use super::playback::*;
use super::state::*;
use super::ui::*;
use crate::{
    commands::cmd_check_music,
    commands::get_call_or_join_author,
    commands::music::gp_prompts::{draw_prompts, GpCategory},
    errors::CrackedError,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    messaging::messages::{GP_SCOREBOARD, SPOTIFY_GP_ONE_SONG, SPOTIFY_NOTHING_PLAYABLE},
    music::queue::{force_skip_top_track, stop_queue},
    music::PlaybackOwner,
    poise_ext::PoiseContextExt,
    sources::sleevenote,
    Context, CrackedResult, Error,
};
use ::serenity::{
    all::{ChannelId, GuildId, UserId},
    builder::CreateMessage,
};
use crack_types::QueryType;
use songbird::Call;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;

// ------------------------------------------------------------------
// Commands
// ------------------------------------------------------------------

async fn author_display_name(ctx: Context<'_>) -> String {
    match ctx.author_member().await {
        Some(m) => m.display_name().to_string(),
        None => ctx.author().name.to_string(),
    }
}

/// Non-bot members of `vc`, from the cache. Empty if the guild isn't cached
/// (then the window simply waits for the timer or the host). The bot is always
/// in this channel and is excluded by id, not just by the member lookup, which
/// answers "human" for anyone the cache is missing.
fn gp_vc_members(ctx: Context<'_>, vc: ChannelId) -> Vec<UserId> {
    // Read before the guild: no cache lock is held across the other lookup.
    let me = ctx.serenity_context().cache.current_user().id;
    let Some(guild) = ctx.guild() else {
        return Vec::new();
    };
    guild
        .voice_states
        .iter()
        .filter(|vs| vs.channel_id == Some(vc))
        .filter(|vs| vs.user_id != me)
        .filter(|vs| guild.members.get(&vs.user_id).is_none_or(|m| !m.user.bot()))
        .map(|vs| vs.user_id)
        .collect()
}

/// The game is for the people in the room: acting on a running game means being in
/// its voice channel. `/gp submit` needs only this much -- submitting is how you
/// join -- while everything else also goes through [`gp_require_player`].
fn gp_require_in_game_vc(ctx: Context<'_>, guild_id: GuildId) -> CrackedResult<ChannelId> {
    let game_vc = ctx
        .data()
        .gp_voice_channel(guild_id)
        .ok_or(CrackedError::NoGameInProgress)?;
    if ctx.author_vc() != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel);
    }
    Ok(game_vc)
}

/// In the voice channel *and* actually playing. `/gp end` is the deliberate
/// exception to both, so an admin can always kill a game from outside it.
fn gp_require_player(ctx: Context<'_>, guild_id: GuildId) -> CrackedResult<ChannelId> {
    let game_vc = gp_require_in_game_vc(ctx, guild_id)?;
    ctx.data().gp_require_player(guild_id, ctx.author().id)?;
    Ok(game_vc)
}

fn gp_playback(ctx: Context<'_>, call: Arc<Mutex<Call>>, guild_id: GuildId) -> GpPlayback {
    GpPlayback {
        data: ctx.data().clone(),
        http: ctx.serenity_context().http.clone(),
        call,
        guild_id,
    }
}

/// "What's your song?" party game: a prompt, secret submissions, guess whose is whose.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    aliases("guiltypleasure"),
    subcommands(
        "gp_start",
        "gp_submit",
        "gp_close",
        "gp_skip",
        "gp_voteskip",
        "gp_votefull",
        "gp_status",
        "gp_end"
    )
)]
pub async fn gp(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send_embed_response(gp_rules_embed()).await?;
    Ok(())
}

/// Start a game in your voice channel: pick a category, rounds, and the submission timer.
#[cfg(not(tarpaulin_include))]
#[allow(clippy::too_many_arguments)]
#[poise::command(
    rename = "start",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_start(
    ctx: Context<'_>,
    #[description = "Prompt category (or Mixed)."] category: GpCategory,
    #[description = "Number of rounds (default 5)."]
    #[min = 1]
    #[max = 20]
    rounds: Option<u32>,
    #[description = "Seconds to submit each round (default 180)."]
    #[min = 30]
    #[max = 600]
    timer: Option<u32>,
    #[description = "Play a clip of each song instead of all of it (default yes)."] clips: Option<
        bool,
    >,
    #[description = "Seconds into each song the clip starts (default 30, needs clips)."]
    #[min = 0]
    #[max = 120]
    clip_start: Option<u32>,
    #[description = "Seconds of each song to play (default 45, needs clips)."]
    #[min = 20]
    #[max = 300]
    clip_length: Option<u32>,
    #[description = "Name submitters only at the end of the round (default), or after each song."]
    reveal: Option<GpReveal>,
    #[description = "Sum each round up in a results embed when it ends (default yes; always on with reveal:round)."]
    results: Option<bool>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    if data.gp_is_active(guild_id) {
        return Err(CrackedError::GameAlreadyRunning.into());
    }
    let vc = ctx.author_vc().ok_or(CrackedError::NotConnected)?;
    let host = ctx.author().id;
    let host_name = author_display_name(ctx).await;

    // Join (or reuse) the call, and make sure it is the host's channel.
    let call = get_call_or_join_author(ctx).await?;
    {
        let handler = call.lock().await;
        if let Some(chan) = handler.current_channel() {
            if chan.get() != vc.get() {
                return Err(CrackedError::WrongVoiceChannel.into());
            }
        }
    }

    let rounds = rounds.unwrap_or(GP_DEFAULT_ROUNDS).clamp(1, GP_MAX_ROUNDS) as usize;
    let timer_secs = timer
        .map(u64::from)
        .unwrap_or(GP_DEFAULT_TIMER_SECS)
        .clamp(GP_MIN_TIMER_SECS, GP_MAX_TIMER_SECS);
    // The two dials only mean anything with clips on; off, they are ignored
    // rather than quietly turning clips back on for a host who asked for whole
    // songs.
    let clip = clips.unwrap_or(GP_DEFAULT_CLIPS).then(|| GpClip {
        start: Duration::from_secs(
            clip_start
                .map(u64::from)
                .unwrap_or(GP_DEFAULT_CLIP_START_SECS)
                .min(GP_MAX_CLIP_START_SECS),
        ),
        length: Duration::from_secs(
            clip_length
                .map(u64::from)
                .unwrap_or(GP_DEFAULT_CLIP_LENGTH_SECS)
                .clamp(GP_MIN_CLIP_LENGTH_SECS, GP_MAX_CLIP_LENGTH_SECS),
        ),
    });
    let reveal = reveal.unwrap_or_default();
    let round_results = results.unwrap_or(true) || reveal == GpReveal::Round;
    let prompts = draw_prompts(category, rounds, &mut rand::rng());

    // Create the game first so the global TrackEndHandler ignores the End
    // event that stopping an existing queue fires.
    let opened = data.gp_start(
        guild_id,
        host,
        host_name,
        vc,
        ctx.channel_id(),
        category,
        prompts,
        timer_secs,
        clip,
        reveal,
        round_results,
        now(),
    )?;
    // `data.gp_start` above claimed the lease, so the game owns playback by the
    // time we reach here and this must lock as `Game`; `Free` would be refused
    // and the previous queue would keep playing underneath the new game.
    // 🪤 Do NOT reorder the claim below this call to make the guild `Free`
    // here: creating the game first is load-bearing, for the reason the comment
    // above `data.gp_start` gives. Locking as `Game` cannot be refused (`as_`
    // matches both `Free` and `Game`), so this `?` cannot orphan the game that
    // was just created.
    let guard = data
        .lock_queue(
            guild_id,
            PlaybackOwner::Game,
            crate::music::audit::Actor::from_ctx(&ctx),
        )
        .await?;
    let cleared_queue = {
        let handler = call.lock().await;
        let non_empty = !handler.queue().is_empty();
        if non_empty {
            #[expect(
                clippy::disallowed_methods,
                reason = "/gp start stops any leftover queue as the game owner before the first round"
            )]
            stop_queue(&guard, &handler);
        }
        non_empty
    };
    // Released before the Discord round trip below -- `stop_queue` fires `End`
    // inline on songbird's event task, which must not wait out a send.
    drop(guard);

    ctx.send_reply(
        CrackedMessage::GpStarted {
            category: category.display(),
            rounds: opened.total_rounds,
            timer_secs,
            clip,
            reveal,
            round_results,
            cleared_queue,
        },
        true,
    )
    .await?;

    let pb = gp_playback(ctx, call, guild_id);
    gp_open_round(&pb, opened).await
}

/// Secretly submit your song for the current prompt (link or search). Resubmitting replaces it.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "submit",
    category = "Games",
    slash_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_submit(
    ctx: Context<'_>,
    #[description = "song link or search query."] query: String,
) -> Result<(), Error> {
    // Errors are answered here, ephemerally, rather than through the framework's
    // public error reply.
    let msg = match gp_submit_internal(ctx, query).await {
        Ok(m) => m,
        Err(e) => refuse_gp("submit", ctx.author().id, ctx.guild_id(), e),
    };
    ctx.send_message(SendMessageParams::new(msg).with_ephemeral(true))
        .await?;
    Ok(())
}

/// Answer a refused `/gp` command, and leave the operator a trace of it (#468).
///
/// The reply stays ephemeral at every call site -- a submission is secret, and
/// a vote names the voter. What was missing is the server-side record: a
/// refusal used to leave nothing in the log. 🔑 Logging and building the reply
/// in one function means a new refusal site gets both or neither.
pub(in crate::commands::music::gp) fn refuse_gp(
    command: &'static str,
    user_id: UserId,
    guild_id: Option<GuildId>,
    err: CrackedError,
) -> CrackedMessage {
    tracing::warn!(command, %user_id, ?guild_id, error = %err, "gp: command refused");
    CrackedMessage::CrackedError(err)
}

/// Resolve a Spotify link for `/gp submit`, which takes exactly one song.
///
/// An album or playlist is refused rather than quietly reduced to its first
/// track: *which* song a player submits is the whole game, so choosing one for
/// them would replace their move with ours and they would never know.
async fn gp_spotify_query(url: &str) -> CrackedResult<QueryType> {
    let resolution = sleevenote::resolve_spotify(url).await?;
    if resolution.media_type.is_collection() {
        return Err(CrackedError::Other(SPOTIFY_GP_ONE_SONG));
    }
    let query = resolution
        .queries()
        .into_iter()
        .next()
        .ok_or(CrackedError::Other(SPOTIFY_NOTHING_PLAYABLE))?;
    Ok(QueryType::Keywords(query))
}

#[cfg(not(tarpaulin_include))]
pub async fn gp_submit_internal(ctx: Context<'_>, query: String) -> CrackedResult<CrackedMessage> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_in_game_vc(ctx, guild_id)?;
    data.gp_window_open(guild_id)?;
    let query = query.trim();
    if query.is_empty() {
        return Err(CrackedError::NoQuery);
    }
    let query_type = match QueryType::from_str(query).map_err(CrackedError::TrackResolveError)? {
        QueryType::SpotifyLink(url) => {
            // Resolving a Spotify link is an HTTP round trip that takes ten
            // seconds or more on a cold cache, well past Discord's
            // three-second interaction deadline -- so defer before doing any
            // of it, or the command fails before the answer exists. Only this
            // path defers: a search or a YouTube link answers fast enough that
            // a "thinking" state would be a downgrade.
            ctx.defer_ephemeral().await?;
            gp_spotify_query(&url).await?
        },
        other => other,
    };
    let track = data
        .ct_client
        .resolve_track(query_type)
        .await
        .map_err(CrackedError::TrackFail)?;
    let name = author_display_name(ctx).await;
    let title = track.get_title();
    let vc_members = gp_vc_members(ctx, game_vc);
    let outcome = data.gp_submit(guild_id, ctx.author().id, name, track, &vc_members)?;

    // Take the call first: closing commits the shuffle and moves the game to
    // `Playing`, so discovering there is nothing to play on afterwards would
    // strand the round with nothing ever enqueued to advance it.
    if outcome.everyone_in {
        // 🪤 `connected_call`, not `Songbird::get` (#507). A Call left behind by
        // a join that never completed satisfied `get`, so the window closed and
        // the round was enqueued into a driver connected to nothing -- the
        // "Queued, then silence" #499 fixed on /play. Not connected, the window
        // now stays open rather than closing onto nothing.
        if let Some(call) = crate::commands::connected_call(&data.songbird, guild_id, None).await {
            if let Some(closed) =
                data.gp_close_window_if(guild_id, outcome.generation, &mut rand::rng(), now())
            {
                let pb = gp_playback(ctx, call, guild_id);
                // Off this task so the ephemeral confirmation isn't held up.
                tokio::spawn(async move {
                    if let Err(e) = gp_after_close(pb, closed).await {
                        tracing::warn!("gp: closing window in {guild_id}: {e}");
                    }
                });
            }
        } else {
            tracing::warn!(
                "gp: everyone is in for {guild_id} but the bot is not connected; \
                 leaving the submission window open"
            );
        }
    }
    Ok(CrackedMessage::GpSubmitted {
        title,
        replaced: outcome.replaced,
        submitted: outcome.submitted,
        of: vc_members.len(),
    })
}

/// Close submissions early and start playing this round (host only).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "close",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_close(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    gp_require_player(ctx, guild_id)?;
    // Take the call *before* closing: `gp_close_window` commits the shuffle and
    // moves the game to `Playing`, and bailing after that would leave the round
    // closed with nothing ever enqueued to advance it.
    let call = crate::commands::connected_call(&data.songbird, guild_id, None)
        .await
        .ok_or(CrackedError::NotConnected)?;
    let closed = data.gp_close_window(guild_id, ctx.author().id, &mut rand::rng(), now())?;
    if let Err(e) = ctx
        .send_reply(
            CrackedMessage::GpWindowClosed {
                count: closed.count,
            },
            true,
        )
        .await
    {
        tracing::warn!("gp: acknowledging the close in {guild_id}: {e}");
    }
    gp_after_close(gp_playback(ctx, call, guild_id), closed).await
}

/// End the current song early (host only).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "skip",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_skip(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    gp_require_player(ctx, guild_id)?;
    {
        let game = data
            .gp_games
            .get(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.host != ctx.author().id {
            return Err(CrackedError::NotGameHost.into());
        }
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying.into());
        }
    }
    let call = crate::commands::connected_call(&data.songbird, guild_id, None)
        .await
        .ok_or(CrackedError::NotConnected)?;
    {
        // Locking as `Game` succeeds here because this guild's game already
        // owns playback and has not released it (see
        // `the_owner_can_still_lock_its_own_queue` in lease.rs); `Free` would
        // be refused and the skip would silently do nothing.
        let guard = data
            .lock_queue(
                guild_id,
                PlaybackOwner::Game,
                crate::music::audit::Actor::from_ctx(&ctx),
            )
            .await?;
        let handler = call.lock().await;
        if handler.queue().is_empty() {
            return Err(CrackedError::NothingPlaying.into());
        }
        // stop() fires TrackEvent::End, which is what advances the game.
        #[expect(
            clippy::disallowed_methods,
            reason = "/gp skip is the game owner moving its own round on"
        )]
        force_skip_top_track(&guard, &handler).await?;
    }
    ctx.send_reply(CrackedMessage::GpRoundSkipped, true).await?;
    Ok(())
}

/// What a vote command says: `mine` goes to the voter alone, `room` -- if there
/// is anything the room should hear -- to the game's channel, naming nobody.
type GpVoteAnswer = (CrackedMessage, Option<CrackedMessage>);

/// Answer a vote. The voter's confirmation is ephemeral, the same as
/// `/gp submit`'s: a slash reply lands under "*name* used `/gp voteskip`", and
/// who wants a song gone is nobody's business in a game whose whole premise is
/// that people submitted something embarrassing. What the room is told goes to
/// the game's channel as a plain message, so the last voter is not named on
/// that either.
async fn gp_answer_vote(ctx: Context<'_>, (mine, room): GpVoteAnswer) -> Result<(), Error> {
    if ctx.is_prefix() {
        // `ephemeral` is an interaction flag: a prefix invocation gets a public
        // reply, and "you already voted to skip this" names the voter as surely
        // as the vote would have. Answer by DM instead, and if that cannot be
        // delivered say nothing in the channel -- the room's line below still
        // says the vote counted. (The `!gp voteskip` message itself is public;
        // the slash form is the one that keeps a vote to yourself.)
        let dm = ctx
            .author()
            .create_dm_channel(&ctx)
            .await
            .map(|c| c.id.widen());
        #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
        let sent = match dm {
            Ok(dm) => dm
                .send_message(
                    &ctx.serenity_context().http,
                    CreateMessage::new().content(mine.to_string()),
                )
                .await
                .map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = sent {
            tracing::warn!(
                "gp: answering a prefix vote by DM in {:?}: {e}",
                ctx.guild_id()
            );
        }
    } else {
        ctx.send_message(SendMessageParams::new(mine).with_ephemeral(true))
            .await?;
    }
    let Some(room) = room else {
        return Ok(());
    };
    // The vote may have ended the game's last song; then there is no game and
    // nothing to tell anyone.
    let Some(channel) = ctx
        .guild_id()
        .and_then(|guild_id| ctx.data().gp_text_channel(guild_id))
    else {
        return Ok(());
    };
    #[expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]
    channel
        .send_message(
            &ctx.serenity_context().http,
            CreateMessage::new().content(room.to_string()),
        )
        .await?;
    Ok(())
}

/// Vote to end the current song early -- a majority of the voice channel ends it.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "voteskip",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_voteskip(ctx: Context<'_>) -> Result<(), Error> {
    // Errors are answered here, ephemerally, rather than through the framework's
    // public error reply: "you already voted to skip this" names the voter as
    // surely as the vote would have.
    let answer = match gp_voteskip_internal(ctx).await {
        Ok(a) => a,
        Err(e) => (
            refuse_gp("voteskip", ctx.author().id, ctx.guild_id(), e),
            None,
        ),
    };
    gp_answer_vote(ctx, answer).await
}

#[cfg(not(tarpaulin_include))]
async fn gp_voteskip_internal(ctx: Context<'_>) -> CrackedResult<GpVoteAnswer> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_player(ctx, guild_id)?;
    let vc_members = gp_vc_members(ctx, game_vc);
    let name = author_display_name(ctx).await;
    let outcome = data.gp_vote_skip(guild_id, ctx.author().id, name, &vc_members)?;
    let answer = match outcome {
        GpVoteSkipOutcome::Counted { votes, needed } => (
            CrackedMessage::GpVoteSkipCounted { votes, needed },
            Some(CrackedMessage::GpVoteSkipRoom { needed }),
        ),
        GpVoteSkipOutcome::Passed => (
            CrackedMessage::GpVoteSkipCarried,
            Some(CrackedMessage::GpVoteSkipPassed),
        ),
        GpVoteSkipOutcome::OwnSong => (
            CrackedMessage::GpVoteSkipOwnSong,
            Some(CrackedMessage::GpVoteSkipPulled),
        ),
    };
    if !matches!(outcome, GpVoteSkipOutcome::Counted { .. }) {
        let call = crate::commands::connected_call(&data.songbird, guild_id, None)
            .await
            .ok_or(CrackedError::NotConnected)?;
        // Locking as `Game` succeeds here because this guild's game already
        // owns playback and has not released it (see
        // `the_owner_can_still_lock_its_own_queue` in lease.rs); `Free` would
        // be refused and the skip would silently do nothing.
        let guard = data
            .lock_queue(
                guild_id,
                PlaybackOwner::Game,
                crate::music::audit::Actor::from_ctx(&ctx),
            )
            .await?;
        let handler = call.lock().await;
        if handler.queue().is_empty() {
            return Err(CrackedError::NothingPlaying);
        }
        // stop() fires TrackEvent::End, which is what advances the game.
        #[expect(
            clippy::disallowed_methods,
            reason = "a /gp vote-skip is the game owner moving its own round on"
        )]
        force_skip_top_track(&guard, &handler).await?;
    }
    Ok(answer)
}

/// Vote to hear the whole song instead of just the clip -- a majority lets it run.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "votefull",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_votefull(ctx: Context<'_>) -> Result<(), Error> {
    // Same split as `/gp voteskip`, for the same reason: one pattern, not two.
    let answer = match gp_votefull_internal(ctx).await {
        Ok(a) => a,
        Err(e) => (
            refuse_gp("votefull", ctx.author().id, ctx.guild_id(), e),
            None,
        ),
    };
    gp_answer_vote(ctx, answer).await
}

#[cfg(not(tarpaulin_include))]
async fn gp_votefull_internal(ctx: Context<'_>) -> CrackedResult<GpVoteAnswer> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_player(ctx, guild_id)?;
    let vc_members = gp_vc_members(ctx, game_vc);
    let name = author_display_name(ctx).await;
    Ok(
        match data.gp_vote_full(guild_id, ctx.author().id, name, &vc_members)? {
            GpVoteFullOutcome::Counted { votes, needed } => (
                CrackedMessage::GpVoteFullCounted { votes, needed },
                Some(CrackedMessage::GpVoteFullRoom { needed }),
            ),
            // Nothing to do to the track: the clip timer checks `play_full` before
            // it stops anything, so letting it run on is simply not stopping it.
            GpVoteFullOutcome::Passed => (
                CrackedMessage::GpVoteFullCarried,
                Some(CrackedMessage::GpVoteFullPassed),
            ),
            // Already carried by an earlier vote: the room heard about it then.
            GpVoteFullOutcome::AlreadyFull => (CrackedMessage::GpVoteFullAlready, None),
        },
    )
}

/// The prompt, who has submitted or guessed, likes, and the scores so far.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "status",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    gp_require_player(ctx, guild_id)?;
    let status = ctx.data().gp_status(guild_id)?;
    ctx.send_embed_response(gp_status_embed(&status)).await?;
    Ok(())
}

/// Abort the game (host, or anyone who can manage the server).
#[cfg(not(tarpaulin_include))]
#[poise::command(
    rename = "end",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_end(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let is_admin = ctx
        .author_permissions()
        .await
        .map(|p| p.manage_guild())
        .unwrap_or(false);
    // Park and stop, but do not remove: `stop()` only queues the `End`, so removing
    // here would beat it to the map and hand the event to autoplay. The global
    // track-end handler collects the parked game when the `End` actually lands.
    let game = data.gp_park_for_end(guild_id, ctx.author().id, is_admin)?;
    // Raw `get` on purpose (#507), unlike `/gp submit`. This is cleanup, and
    // stopping the queue must reach any registered Call, connected or not:
    // songbird reuses a guild's Call on the next join, so tracks left in a
    // stranded one would start playing in whatever comes next.
    #[expect(
        clippy::disallowed_methods,
        reason = "raw Songbird::get on purpose (#507: cleanup reaches any registered Call), and the game owner stopping its own queue"
    )]
    let was_playing = match data.songbird.get(guild_id) {
        Some(call) => {
            // `gp_park_for_end` only sets a flag -- the game stays in the map,
            // and with it the lease, until `gp_remove_if_parked` collects it in
            // the track-end handler. So the game still owns playback here and
            // this locks as `Game`; `Free` would be refused and the queue would
            // never stop, leaving the parked game with no `End` to collect it.
            let guard = data
                .lock_queue(
                    guild_id,
                    PlaybackOwner::Game,
                    crate::music::audit::Actor::from_ctx(&ctx),
                )
                .await?;
            let handler = call.lock().await;
            let playing = !handler.queue().is_empty();
            stop_queue(&guard, &handler);
            playing
            // Both locks drop with this arm, before the replies below.
        },
        None => false,
    };
    if was_playing {
        // If that `End` never reaches the handler, the parked game would outlive
        // the command and keep the guild's music commands blocked.
        let data = data.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(GP_PARK_GRACE_SECS)).await;
            if data.gp_remove_if_parked(guild_id) {
                tracing::warn!("gp: parked game in {guild_id} was never collected, removing");
            }
        });
    } else {
        // Nothing was playing, so no `End` is coming to collect it.
        data.gp_remove(guild_id);
    }
    let by = author_display_name(ctx).await;
    ctx.send_reply(CrackedMessage::GpEnded { by }, true).await?;
    let nothing_played = game.phase == GpPhase::Submitting && game.current_round == 0;
    if !nothing_played {
        ctx.send_embed_response(gp_scoreboard_embed(&game.sorted_scores(), GP_SCOREBOARD))
            .await?;
    }
    Ok(())
}
