//! The seam between the routes and the bot. Routes are generic over
//! [`Backend`], so their logic is tested against a fake; `LiveBackend`
//! (lib.rs) is the thin glue over crack-core, the cache and Discord.

use crate::{
    access::{HistoryAccess, Presence},
    history::{HistoryPage, HistoryQuery},
    settings::{
        EconomySaveRefused, EconomySettingsForm, EconomySettingsView, ModLogSaveRefused,
        ModLogSettingsForm, ModLogSettingsView, RoomSettingsForm, RoomSettingsView, SaveRefused,
    },
    view::PlanView,
    watch::ViewSource,
};
use chrono::{DateTime, Utc};
pub use crack_core::music::remote::{AddRefused, At, Control, ControlRefused, MoveRefused};
pub use crack_core::music::search::{Query, SavedTrack, Source, SourceResult, SourceSet};
use crack_model::{CommunityRef, UserRef};
use std::future::Future;
use uuid::Uuid;

/// A guild on the picker.
#[derive(Debug, Clone)]
pub struct GuildEntry {
    pub id: CommunityRef,
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
    fn presence(&self, g: CommunityRef, u: UserRef) -> impl Future<Output = Presence> + Send;
    fn move_track(
        &self,
        user: UserRef,
        g: CommunityRef,
        id: Uuid,
        to: usize,
    ) -> impl Future<Output = Result<usize, MoveRefused>> + Send;
    fn control(
        &self,
        user: UserRef,
        g: CommunityRef,
        c: Control,
    ) -> impl Future<Output = Result<(), ControlRefused>> + Send;
    fn plan(&self, g: CommunityRef) -> impl Future<Output = PlanView> + Send;
    fn guilds_for(&self, u: UserRef) -> impl Future<Output = Vec<GuildEntry>> + Send;
    fn guild_name(&self, g: CommunityRef) -> Option<String>;
    fn history_access(
        &self,
        g: CommunityRef,
        u: UserRef,
    ) -> impl Future<Output = HistoryAccess> + Send;
    fn history(
        &self,
        g: CommunityRef,
        q: &HistoryQuery,
    ) -> impl Future<Output = Result<HistoryPage, HistoryError>> + Send;
    fn search(
        &self,
        g: CommunityRef,
        q: &Query,
        sources: SourceSet,
        since: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Vec<(Source, SourceResult)>> + Send;
    fn add(
        &self,
        user: UserRef,
        name: &str,
        g: CommunityRef,
        track: SavedTrack,
        at: At,
    ) -> impl Future<Output = Result<(), AddRefused>> + Send;
    fn room_settings(&self, g: CommunityRef) -> impl Future<Output = RoomSettingsView> + Send;
    fn save_room_settings(
        &self,
        user: UserRef,
        g: CommunityRef,
        f: RoomSettingsForm,
    ) -> impl Future<Output = Result<RoomSettingsView, SaveRefused>> + Send;
    fn mod_log_settings(&self, g: CommunityRef) -> impl Future<Output = ModLogSettingsView> + Send;
    fn save_mod_log_settings(
        &self,
        user: UserRef,
        g: CommunityRef,
        f: ModLogSettingsForm,
    ) -> impl Future<Output = Result<ModLogSettingsView, ModLogSaveRefused>> + Send;
    fn economy_settings(&self, g: CommunityRef)
        -> impl Future<Output = EconomySettingsView> + Send;
    fn save_economy_settings(
        &self,
        user: UserRef,
        g: CommunityRef,
        f: EconomySettingsForm,
    ) -> impl Future<Output = Result<EconomySettingsView, EconomySaveRefused>> + Send;
}
