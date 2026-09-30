//! The seam between the routes and the bot. Routes are generic over
//! [`Backend`], so their logic is tested against a fake; `LiveBackend`
//! (lib.rs) is the thin glue over crack-core, the cache and Discord.

use crate::{access::Presence, watch::ViewSource};
pub use crack_core::music::remote::MoveRefused;
use serenity::all::{GuildId, UserId};
use std::future::Future;
use uuid::Uuid;

/// A guild on the picker.
#[derive(Debug, Clone)]
pub struct GuildEntry {
    pub id: GuildId,
    pub name: String,
    /// The voice channel the bot is in, by name.
    pub channel: Option<String>,
}

pub trait Backend: ViewSource {
    fn presence(&self, g: GuildId, u: UserId) -> impl Future<Output = Presence> + Send;
    fn move_track(
        &self,
        g: GuildId,
        id: Uuid,
        to: usize,
    ) -> impl Future<Output = Result<usize, MoveRefused>> + Send;
    fn guilds_for(&self, u: UserId) -> impl Future<Output = Vec<GuildEntry>> + Send;
    fn guild_name(&self, g: GuildId) -> Option<String>;
}
