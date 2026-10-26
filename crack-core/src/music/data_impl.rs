//! `Data`'s side of the music player: each method hands the call to the
//! [`MusicData`](super::MusicData) handle, which shares the maps.

use super::lease::{PlaybackOwner, QueueGuard};
use crate::{errors::CrackedError, Data};
use serenity::all::GuildId;

impl Data {
    /// Who owns playback in this guild. A guild with no claim is [`PlaybackOwner::Free`].
    #[must_use]
    pub fn playback_owner(&self, guild_id: GuildId) -> PlaybackOwner {
        self.music_data().playback_owner(guild_id)
    }

    /// Take ownership of playback in a guild; see `MusicData::claim_playback`.
    ///
    /// # Errors
    ///
    /// [`CrackedError::GameInProgress`] if a *different* owner already holds it.
    pub fn claim_playback(
        &self,
        guild_id: GuildId,
        owner: PlaybackOwner,
    ) -> Result<(), CrackedError> {
        self.music_data().claim_playback(guild_id, owner)
    }

    /// Give up ownership of playback in a guild.
    pub fn release_playback(&self, guild_id: GuildId) {
        self.music_data().release_playback(guild_id);
    }

    /// Acquire exclusive permission to mutate this guild's queue; see
    /// `MusicData::lock_queue`.
    ///
    /// # Errors
    ///
    /// [`CrackedError::GameInProgress`] when a game owns playback and `as_` is
    /// not that owner.
    pub async fn lock_queue(
        &self,
        guild_id: GuildId,
        as_: PlaybackOwner,
        actor: crate::music::audit::Actor,
    ) -> Result<QueueGuard, CrackedError> {
        self.music_data().lock_queue(guild_id, as_, actor).await
    }

    /// Get the current autoplay settings.
    pub async fn get_autoplay(&self, guild_id: GuildId) -> bool {
        self.music_data().get_autoplay(guild_id).await
    }

    /// Turn autoplay on or off for this guild, for this session.
    pub async fn set_autoplay(&self, guild_id: GuildId, autoplay: bool) {
        self.music_data().set_autoplay(guild_id, autoplay).await;
    }
}
