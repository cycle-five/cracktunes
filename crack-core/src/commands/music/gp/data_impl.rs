//! `Data`'s side of the /gp game: each method hands the call to the
//! [`GpData`](super::GpData) handle, which shares the games map and the lease.

use crate::commands::music::gp_prompts::{GpCategories, GpPrompt};
use crate::{CrackedResult, Data};
use ::serenity::all::{ChannelId, GenericChannelId, GuildId, MessageId, UserId};
use crack_gp::gp::{
    GpClip, GpGame, GpGuessOutcome, GpLikeOutcome, GpReveal, GpStatus, GpSubmitOutcome,
    GpTrackResult, GpTrackStart, GpVoteFullOutcome, GpVoteSkipOutcome, GpWindowClosed,
    GpWindowOpened, GpWindowWarning,
};
use crack_testing::ResolvedTrack;
use rand::Rng;
use std::time::Duration;

impl Data {
    #[allow(clippy::too_many_arguments)]
    pub fn gp_start(
        &self,
        guild_id: GuildId,
        host: UserId,
        host_name: String,
        voice_channel: ChannelId,
        text_channel: GenericChannelId,
        categories: GpCategories,
        prompts: Vec<GpPrompt>,
        timer_secs: u64,
        clip: Option<GpClip>,
        reveal: GpReveal,
        round_results: bool,
        now: i64,
    ) -> CrackedResult<GpWindowOpened> {
        self.gp_data().gp_start(
            guild_id,
            host,
            host_name,
            voice_channel,
            text_channel,
            categories,
            prompts,
            timer_secs,
            clip,
            reveal,
            round_results,
            now,
        )
    }

    pub fn gp_window_open(&self, guild_id: GuildId) -> CrackedResult<()> {
        self.gp_data().gp_window_open(guild_id)
    }

    pub fn gp_submit(
        &self,
        guild_id: GuildId,
        user: UserId,
        name: String,
        track: ResolvedTrack<'static>,
        vc_members: &[UserId],
    ) -> CrackedResult<GpSubmitOutcome> {
        self.gp_data()
            .gp_submit(guild_id, user, name, track, vc_members)
    }

    pub fn gp_close_window(
        &self,
        guild_id: GuildId,
        caller: UserId,
        rng: &mut impl Rng,
        now: i64,
    ) -> CrackedResult<GpWindowClosed> {
        self.gp_data().gp_close_window(guild_id, caller, rng, now)
    }

    pub fn gp_close_window_if(
        &self,
        guild_id: GuildId,
        generation: u64,
        rng: &mut impl Rng,
        now: i64,
    ) -> Option<GpWindowClosed> {
        self.gp_data()
            .gp_close_window_if(guild_id, generation, rng, now)
    }

    pub fn gp_warning_if(&self, guild_id: GuildId, generation: u64) -> Option<GpWindowWarning> {
        self.gp_data().gp_warning_if(guild_id, generation)
    }

    pub fn gp_set_prompt_message(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        channel: GenericChannelId,
        message_id: MessageId,
    ) -> CrackedResult<()> {
        self.gp_data()
            .gp_set_prompt_message(guild_id, round_idx, channel, message_id)
    }

    pub fn gp_set_track_message(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        channel: GenericChannelId,
        message_id: MessageId,
    ) -> CrackedResult<()> {
        self.gp_data()
            .gp_set_track_message(guild_id, round_idx, track_idx, channel, message_id)
    }

    pub fn gp_record_guess(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        guesser: UserId,
        guesser_name: String,
        guessed: UserId,
    ) -> CrackedResult<GpGuessOutcome> {
        self.gp_data().gp_record_guess(
            guild_id,
            round_idx,
            track_idx,
            guesser,
            guesser_name,
            guessed,
        )
    }

    pub fn gp_toggle_like(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        liker: UserId,
        liker_name: String,
    ) -> CrackedResult<GpLikeOutcome> {
        self.gp_data()
            .gp_toggle_like(guild_id, round_idx, track_idx, liker, liker_name)
    }

    pub fn gp_vote_skip(
        &self,
        guild_id: GuildId,
        voter: UserId,
        voter_name: String,
        vc_members: &[UserId],
    ) -> CrackedResult<GpVoteSkipOutcome> {
        self.gp_data()
            .gp_vote_skip(guild_id, voter, voter_name, vc_members)
    }

    pub fn gp_vote_full(
        &self,
        guild_id: GuildId,
        voter: UserId,
        voter_name: String,
        vc_members: &[UserId],
    ) -> CrackedResult<GpVoteFullOutcome> {
        self.gp_data()
            .gp_vote_full(guild_id, voter, voter_name, vc_members)
    }

    pub fn gp_clip_still_current(
        &self,
        guild_id: GuildId,
        generation: u64,
        round_idx: usize,
        track_idx: usize,
    ) -> bool {
        self.gp_data()
            .gp_clip_still_current(guild_id, generation, round_idx, track_idx)
    }

    pub fn gp_plays_full(&self, guild_id: GuildId, round_idx: usize, track_idx: usize) -> bool {
        self.gp_data().gp_plays_full(guild_id, round_idx, track_idx)
    }

    pub fn gp_heard_by_vote(&self, guild_id: GuildId, round_idx: usize, track_idx: usize) -> bool {
        self.gp_data()
            .gp_heard_by_vote(guild_id, round_idx, track_idx)
    }

    pub fn gp_reveal_and_advance(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        now: i64,
    ) -> Option<GpTrackResult> {
        self.gp_data()
            .gp_reveal_and_advance(guild_id, round_idx, track_idx, now)
    }

    pub fn gp_cut_short_and_advance(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        now: i64,
    ) -> Option<GpTrackResult> {
        self.gp_data()
            .gp_cut_short_and_advance(guild_id, round_idx, track_idx, now)
    }

    pub fn gp_fail_and_advance(
        &self,
        guild_id: GuildId,
        round_idx: usize,
        track_idx: usize,
        now: i64,
    ) -> Option<GpTrackResult> {
        self.gp_data()
            .gp_fail_and_advance(guild_id, round_idx, track_idx, now)
    }

    pub fn gp_park_for_end(
        &self,
        guild_id: GuildId,
        caller: UserId,
        caller_is_admin: bool,
    ) -> CrackedResult<GpGame> {
        self.gp_data()
            .gp_park_for_end(guild_id, caller, caller_is_admin)
    }

    pub fn gp_remove_if_parked(&self, guild_id: GuildId) -> bool {
        self.gp_data().gp_remove_if_parked(guild_id)
    }

    pub fn gp_collect_parked(&self, guild_id: GuildId, started_at: i64) -> bool {
        self.gp_data().gp_collect_parked(guild_id, started_at)
    }

    pub fn gp_remove(&self, guild_id: GuildId) -> Option<GpGame> {
        self.gp_data().gp_remove(guild_id)
    }

    pub fn gp_restore(&self, guild_id: GuildId, game: GpGame) -> bool {
        self.gp_data().gp_restore(guild_id, game)
    }

    pub fn gp_resume_playing(
        &self,
        guild_id: GuildId,
    ) -> Option<(GpTrackStart, Option<(GenericChannelId, MessageId)>)> {
        self.gp_data().gp_resume_playing(guild_id)
    }

    pub fn gp_mark_results_posted(&self, guild_id: GuildId, round_idx: usize) {
        self.gp_data().gp_mark_results_posted(guild_id, round_idx)
    }

    pub fn gp_require_player(&self, guild_id: GuildId, user: UserId) -> CrackedResult<()> {
        self.gp_data().gp_require_player(guild_id, user)
    }

    pub fn gp_status(&self, guild_id: GuildId) -> CrackedResult<GpStatus> {
        self.gp_data().gp_status(guild_id)
    }

    pub fn gp_is_active(&self, guild_id: GuildId) -> bool {
        self.gp_data().gp_is_active(guild_id)
    }

    pub fn gp_voice_channel(&self, guild_id: GuildId) -> Option<ChannelId> {
        self.gp_data().gp_voice_channel(guild_id)
    }

    pub fn gp_text_channel(&self, guild_id: GuildId) -> Option<GenericChannelId> {
        self.gp_data().gp_text_channel(guild_id)
    }

    pub async fn gp_shutdown(&self, timeout: Duration) {
        self.gp_data().gp_shutdown(timeout).await
    }
}
