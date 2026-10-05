//! The seam between the routes and the bot. Routes are generic over
//! [`Backend`], so their logic is tested against a fake; `LiveBackend`
//! (lib.rs) is the thin glue over crack-core, the cache and Discord.

use crate::{
    access::{HistoryAccess, Presence},
    history::{HistoryPage, HistoryQuery},
    view::PlanView,
    watch::ViewSource,
};
pub use crack_core::music::remote::{Control, ControlRefused, MoveRefused};
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

/// Why a history page could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryError {
    /// The bot runs without a database: there is no history to read.
    NoDatabase,
    /// The query failed; logged where it happened.
    Failed,
}

pub trait Backend: ViewSource {
    fn presence(&self, g: GuildId, u: UserId) -> impl Future<Output = Presence> + Send;
    fn move_track(
        &self,
        user: UserId,
        g: GuildId,
        id: Uuid,
        to: usize,
    ) -> impl Future<Output = Result<usize, MoveRefused>> + Send;
    fn control(
        &self,
        user: UserId,
        g: GuildId,
        c: Control,
    ) -> impl Future<Output = Result<(), ControlRefused>> + Send;
    fn plan(&self, g: GuildId) -> impl Future<Output = PlanView> + Send;
    fn guilds_for(&self, u: UserId) -> impl Future<Output = Vec<GuildEntry>> + Send;
    fn guild_name(&self, g: GuildId) -> Option<String>;
    fn history_access(&self, g: GuildId, u: UserId) -> impl Future<Output = HistoryAccess> + Send;
    fn history(
        &self,
        g: GuildId,
        q: &HistoryQuery,
    ) -> impl Future<Output = Result<HistoryPage, HistoryError>> + Send;
}
