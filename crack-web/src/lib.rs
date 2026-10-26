//! The Crack Tunes web dashboard (arc 1): view a guild's queue live, and
//! reorder it from the bot's voice channel. Runs inside the bot process; see
//! docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md.

pub mod access;
pub mod backend;
pub mod config;
pub mod find;
pub mod history;
pub mod hits;
pub mod limit;
pub mod page;
pub mod routes;
pub mod settings;
#[cfg(test)]
mod test_support;
pub mod view;
pub mod watch;

use crate::{
    access::{HistoryAccess, MemberMemo, Membership, Presence, MEMBER_TTL},
    backend::{
        AddRefused, At, Backend, Control, ControlRefused, GuildEntry, HistoryError, MoveRefused,
        Query, SavedTrack, Source, SourceResult, SourceSet,
    },
    config::WebEnv,
    history::{HistoryPage, HistoryQuery},
    routes::WebState,
    settings::{
        ChannelOption, EconomySaveRefused, EconomySettingsForm, EconomySettingsView,
        ModLogSaveRefused, ModLogSettingsForm, ModLogSettingsView, RoomSettingsForm,
        RoomSettingsView, SaveRefused,
    },
    view::{view_from_state, PlanView, QueueView},
    watch::{Hub, ViewSource, LINGER, TICK},
};
use chrono::{DateTime, Utc};
use crack_core::{
    db::queue_audit::{audit_page, AuditFilter},
    guild::{operations::GuildSettingsOperations, plan::Plan},
    music::remote,
    owners, Data,
};
use crack_model::{CommunityRef, UserRef};
use crack_types::discord::{guild_id, user_id, ToRef};
use serenity::all::{Cache, ChannelId, ChannelType, GuildId, Http, Permissions, RoleId, UserId};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use uuid::Uuid;

/// What the bot hands the dashboard.
pub struct WebDeps {
    pub data: Arc<Data>,
    pub cache: Arc<Cache>,
    pub http: Arc<Http>,
}

/// The real backend: crack-core's `remote`, the cache, and Discord.
pub struct LiveBackend {
    deps: WebDeps,
    memo: MemberMemo,
    roles: access::RoleMemo,
    names: history::NameMemo,
    /// The bot owners, read once: the application's owners come from Discord.
    owners: tokio::sync::OnceCell<HashSet<UserId>>,
}

impl LiveBackend {
    /// Whether `u` is a bot owner, as `owners_only` commands see it.
    async fn is_owner(&self, u: UserId) -> bool {
        self.owners
            .get_or_init(|| owners::bot_owners(&self.deps.data.bot_settings, &self.deps.http))
            .await
            .contains(&u)
    }

    /// The voice channels of `g` from the cache, in the order Discord shows
    /// them, and `hub`'s permission findings as plain text (channel names).
    fn rooms_cache_view(
        &self,
        g: GuildId,
        hub: Option<ChannelId>,
    ) -> (Vec<ChannelOption>, Option<Vec<String>>) {
        use crack_core::rooms::{card::finding_text, preflight};
        let channels = self
            .deps
            .cache
            .guild(g)
            .map(|guild| {
                let mut voice: Vec<_> = guild
                    .channels
                    .iter()
                    .filter(|c| c.base.kind == ChannelType::Voice)
                    .map(|c| (c.position, c.id, c.base.name.to_string()))
                    .collect();
                voice.sort();
                voice
                    .into_iter()
                    .map(|(_, id, name)| ChannelOption {
                        id: id.to_string(),
                        name,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let findings = hub
            .and_then(|h| preflight::for_guild(&self.deps.cache, g, h))
            .map(|found| {
                let name_of = |c: ChannelId| {
                    self.deps
                        .cache
                        .guild(g)
                        .and_then(|guild| guild.channels.get(&c).map(|ch| ch.base.name.to_string()))
                        .unwrap_or_else(|| format!("channel {c}"))
                };
                found
                    .iter()
                    .map(|f| finding_text(f, name_of, false))
                    .collect()
            });
        (channels, findings)
    }

    /// `raw` as a voice channel of `g` in the cache. Ids that serenity
    /// cannot hold (0, u64::MAX) are refused before `ChannelId::new`.
    fn voice_channel_of(&self, g: GuildId, raw: &str) -> Option<ChannelId> {
        let n: u64 = raw.parse().ok().filter(|n| *n != 0 && *n != u64::MAX)?;
        let id = ChannelId::new(n);
        let guild = self.deps.cache.guild(g)?;
        (guild.channels.get(&id)?.base.kind == ChannelType::Voice).then_some(id)
    }

    /// `raw` as a text channel of `g` in the cache; ids serenity cannot hold
    /// (0, u64::MAX) are refused before `ChannelId::new`.
    fn text_channel_of(&self, g: GuildId, raw: &str) -> Option<ChannelId> {
        let n: u64 = raw.parse().ok().filter(|n| *n != 0 && *n != u64::MAX)?;
        let id = ChannelId::new(n);
        let guild = self.deps.cache.guild(g)?;
        (guild.channels.get(&id)?.base.kind == ChannelType::Text).then_some(id)
    }

    fn member_name(&self, g: GuildId, u: UserId) -> Option<String> {
        let guild = self.deps.cache.guild(g)?;
        guild.members.get(&u).map(|m| m.display_name().to_owned())
    }

    /// A member's role ids: the cache, then the role memo (which the
    /// membership lookup in `presence` fills), then Discord.
    /// `None` when Discord did not answer.
    async fn member_roles(&self, g: GuildId, u: UserId) -> Option<Vec<RoleId>> {
        if let Some(roles) = self.deps.cache.guild(g).and_then(|guild| {
            guild
                .members
                .get(&u)
                .map(|m| m.roles.iter().copied().collect::<Vec<_>>())
        }) {
            return Some(roles);
        }
        let now = std::time::Instant::now();
        if let Some(roles) = self.roles.get(g, u, now) {
            return Some(roles);
        }
        match self.deps.http.get_member(g, u).await {
            Ok(m) => {
                let roles: Vec<RoleId> = m.roles.iter().copied().collect();
                self.roles.record(g, u, roles.clone(), now);
                Some(roles)
            },
            Err(e) => {
                tracing::warn!("history: could not read {u}'s roles in {g}: {e}");
                None
            },
        }
    }

    /// Names for `members` (see [`history::shown_members`]): the cache, the
    /// name memo, then at most `NAME_LOOKUPS` Discord lookups. Anyone left
    /// out shows as their id.
    async fn names_for(&self, g: GuildId, members: &[UserId]) -> HashMap<UserId, String> {
        let mut out = HashMap::new();
        let mut lookups = 0;
        for &u in members {
            if let Some(name) = self.member_name(g, u) {
                out.insert(u, name);
                continue;
            }
            let now = std::time::Instant::now();
            if let Some(name) = self.names.get(u, now) {
                out.insert(u, name);
                continue;
            }
            if lookups >= history::NAME_LOOKUPS {
                continue;
            }
            lookups += 1;
            match self.deps.http.get_user(u).await {
                Ok(user) => {
                    let name = user.display_name().to_owned();
                    self.names.record(u, name.clone(), now);
                    out.insert(u, name);
                },
                Err(e) => tracing::warn!("history: could not look up {u}'s name: {e}"),
            }
        }
        out
    }
}

/// The settings pages of a guild that is not a Discord snowflake: nothing is
/// stored for it, so they read as a guild that never configured anything.
/// Only the `off` defaults are read from the placeholder id.
const NOT_A_GUILD: GuildId = GuildId::new(1);

fn room_settings_off() -> RoomSettingsView {
    let s = crack_core::rooms::settings::RoomSettings::off(NOT_A_GUILD);
    RoomSettingsView {
        enabled: s.enabled,
        hub: None,
        name_template: s.name_template,
        default_limit: s.default_limit,
        empty_grace_secs: s.empty_grace_secs,
        voice_channels: Vec::new(),
        findings: None,
    }
}

fn mod_log_settings_off() -> ModLogSettingsView {
    ModLogSettingsView {
        enabled: false,
        channel: None,
        text_channels: Vec::new(),
        findings: None,
        visibility: None,
    }
}

fn economy_settings_off() -> EconomySettingsView {
    EconomySettingsView::from_settings(
        &crack_core::economy::settings::EconomySettings::off(NOT_A_GUILD),
        crack_core::COIN_BUILD,
    )
}

impl ViewSource for LiveBackend {
    async fn view(&self, g: CommunityRef) -> QueueView {
        let Ok(g) = guild_id(&g) else {
            return QueueView::Hidden;
        };
        let state = remote::queue_state(&self.deps.data.music_data(), g).await;
        view_from_state(state, |u| self.member_name(g, u))
    }
}

impl Backend for LiveBackend {
    async fn presence(&self, g: CommunityRef, u: UserRef) -> Presence {
        let (Ok(g), Ok(u)) = (guild_id(&g), user_id(&u)) else {
            return Presence {
                membership: Membership::NotMember,
                user_channel: None,
                bot_channel: None,
            };
        };
        let bot = remote::bot_channel(&self.deps.data.music_data(), g).await;
        access::presence(
            &self.deps.cache,
            &self.deps.http,
            &self.memo,
            &self.roles,
            g,
            u,
            bot,
        )
        .await
    }

    async fn move_track(
        &self,
        user: UserRef,
        g: CommunityRef,
        id: Uuid,
        to: usize,
    ) -> Result<usize, MoveRefused> {
        let (Ok(g), Ok(user)) = (guild_id(&g), user_id(&user)) else {
            return Err(MoveRefused::NotPlaying);
        };
        remote::move_by_id(
            self.deps.data.music_data(),
            self.deps.http.clone(),
            self.deps.cache.clone(),
            g,
            user,
            id,
            to,
        )
        .await
    }

    // Live Discord glue: the cache and HTTP calls cannot be driven in a test.
    #[cfg_attr(test, mutants::skip)]
    async fn control(
        &self,
        user: UserRef,
        g: CommunityRef,
        c: Control,
    ) -> Result<(), ControlRefused> {
        let (Ok(g), Ok(user)) = (guild_id(&g), user_id(&user)) else {
            return Err(ControlRefused::NotPlaying);
        };
        // The echo line is already spawned in core.
        remote::control(
            self.deps.data.music_data(),
            self.deps.http.clone(),
            self.deps.cache.clone(),
            g,
            user,
            crack_core::messaging::cards::Via::Dashboard,
            c,
        )
        .await
        .map(|_| ())
    }

    async fn plan(&self, g: CommunityRef) -> PlanView {
        let Ok(g) = guild_id(&g) else {
            return PlanView::Free;
        };
        crate::view::plan_view(self.deps.data.get_premium(g).await)
    }

    async fn guilds_for(&self, u: UserRef) -> Vec<GuildEntry> {
        let Ok(u) = user_id(&u) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (g, channel) in remote::active_guilds(&self.deps.data.music_data()).await {
            let p = access::presence(
                &self.deps.cache,
                &self.deps.http,
                &self.memo,
                &self.roles,
                g,
                u,
                Some(channel),
            )
            .await;
            if p.membership != Membership::Member {
                continue;
            }
            let (name, channel) = match self.deps.cache.guild(g) {
                Some(guild) => (
                    guild.name.to_string(),
                    guild
                        .channels
                        .get(&channel)
                        .map(|c| c.base.name.to_string()),
                ),
                None => continue,
            };
            out.push(GuildEntry {
                id: g.to_ref(),
                name,
                channel,
            });
        }
        out
    }

    // Live Discord glue: the cache and HTTP calls cannot be driven in a test.
    #[cfg_attr(test, mutants::skip)]
    fn guild_name(&self, g: CommunityRef) -> Option<String> {
        let g = guild_id(&g).ok()?;
        self.deps.cache.guild(g).map(|guild| guild.name.to_string())
    }

    async fn history_access(&self, g: CommunityRef, u: UserRef) -> HistoryAccess {
        let (Ok(g), Ok(u)) = (guild_id(&g), user_id(&u)) else {
            return HistoryAccess::Hidden;
        };
        // Before any Discord call: an owner needs none.
        if self.is_owner(u).await {
            return access::decide_history(true, Membership::Unknown, None);
        }
        let membership = self.presence(g.to_ref(), u.to_ref()).await.membership;
        if membership != Membership::Member {
            return access::decide_history(false, membership, None);
        }
        let roles = self.member_roles(g, u).await;
        let manages = roles.and_then(|roles| {
            let guild = self.deps.cache.guild(g)?;
            let everyone = guild
                .roles
                .get(&RoleId::new(g.get()))
                .map(|r| r.permissions)
                .unwrap_or_else(Permissions::empty);
            Some(access::has_manage_guild(
                guild.owner_id,
                u,
                everyone,
                &roles,
                |r| guild.roles.get(&r).map(|role| role.permissions),
            ))
        });
        access::decide_history(false, membership, manages)
    }

    async fn search(
        &self,
        g: CommunityRef,
        q: &Query,
        sources: SourceSet,
        since: Option<DateTime<Utc>>,
    ) -> Vec<(Source, SourceResult)> {
        let Ok(g) = guild_id(&g) else {
            return Vec::new();
        };
        crack_core::music::search::search(&self.deps.data.music_data(), g, q, sources, since).await
    }

    // Live Discord glue: the cache and HTTP calls cannot be driven in a test.
    #[cfg_attr(test, mutants::skip)]
    async fn add(
        &self,
        user: UserRef,
        name: &str,
        g: CommunityRef,
        track: SavedTrack,
        at: At,
    ) -> Result<(), AddRefused> {
        let (Ok(g), Ok(user)) = (guild_id(&g), user_id(&user)) else {
            return Err(AddRefused::NotPlaying);
        };
        // The echo line is already spawned in core.
        remote::add(
            self.deps.data.music_data(),
            self.deps.http.clone(),
            self.deps.cache.clone(),
            g,
            user,
            name.to_owned(),
            crack_core::messaging::cards::Via::Dashboard,
            track,
            at,
        )
        .await
        .map(|_| ())
    }

    async fn room_settings(&self, g: CommunityRef) -> RoomSettingsView {
        let Ok(g) = guild_id(&g) else {
            return room_settings_off();
        };
        let s = self.deps.data.rooms.settings(g);
        let (voice_channels, findings) = self.rooms_cache_view(g, s.hub);
        RoomSettingsView {
            enabled: s.enabled,
            hub: s.hub.map(|h| h.to_string()),
            name_template: s.name_template,
            default_limit: s.default_limit,
            empty_grace_secs: s.empty_grace_secs,
            voice_channels,
            findings,
        }
    }

    async fn save_room_settings(
        &self,
        user: UserRef,
        g: CommunityRef,
        f: RoomSettingsForm,
    ) -> Result<RoomSettingsView, SaveRefused> {
        let (Ok(g), Ok(user)) = (guild_id(&g), user_id(&user)) else {
            return Err(SaveRefused::NotLoaded);
        };
        use crack_core::rooms::{
            ops::{save_settings, SettingsSaveError},
            settings::RoomSettings,
        };
        let hub = match f.hub.as_deref() {
            Some(raw) => Some(
                self.voice_channel_of(g, raw)
                    .ok_or(SaveRefused::UnknownChannel)?,
            ),
            None => None,
        };
        let settings = RoomSettings {
            guild: g,
            enabled: f.enabled,
            hub,
            name_template: f.name_template,
            default_limit: f.default_limit,
            empty_grace_secs: f.empty_grace_secs,
        };
        save_settings(&self.deps.data.rooms_data(), settings)
            .await
            .map_err(|e| match e {
                SettingsSaveError::Invalid(e) => SaveRefused::Invalid(e),
                SettingsSaveError::Db => SaveRefused::Failed,
                SettingsSaveError::NotLoaded => SaveRefused::NotLoaded,
            })?;
        tracing::info!(guild = %g, user = %user, "dashboard saved room settings");
        Ok(self.room_settings(g.to_ref()).await)
    }

    async fn mod_log_settings(&self, g: CommunityRef) -> ModLogSettingsView {
        let Ok(g) = guild_id(&g) else {
            return mod_log_settings_off();
        };
        use crack_core::{
            messaging::messages::{MODLOG_STATUS_VISIBLE_PREFIX, MODLOG_STATUS_VISIBLE_SUFFIX},
            modlog::preflight::{finding_text, for_guild},
        };
        let s = self.deps.data.modlog.settings(g);
        let (mut text_channels, name) = match self.deps.cache.guild(g) {
            Some(guild) => {
                let mut text: Vec<_> = guild
                    .channels
                    .iter()
                    .filter(|c| c.base.kind == ChannelType::Text)
                    .map(|c| (c.position, c.id, c.base.name.to_string()))
                    .collect();
                text.sort();
                let name = s
                    .channel
                    .and_then(|ch| guild.channels.get(&ch).map(|c| c.base.name.to_string()));
                (text, name)
            },
            None => (Vec::new(), None),
        };
        let name_of = |c: ChannelId| {
            self.deps
                .cache
                .guild(g)
                .and_then(|guild| guild.channels.get(&c).map(|ch| ch.base.name.to_string()))
                .unwrap_or_else(|| format!("channel {c}"))
        };
        let findings = s
            .channel
            .and_then(|_| for_guild(&self.deps.cache, &self.deps.data.rooms_data(), g))
            .map(|found| {
                found
                    .iter()
                    .map(|f| finding_text(f, name_of, false))
                    .collect()
            });
        let visibility = name
            .map(|n| format!("{MODLOG_STATUS_VISIBLE_PREFIX} #{n} {MODLOG_STATUS_VISIBLE_SUFFIX}"));
        ModLogSettingsView {
            enabled: s.enabled,
            channel: s.channel.map(|c| c.to_string()),
            text_channels: text_channels
                .drain(..)
                .map(|(_, id, name)| ChannelOption {
                    id: id.to_string(),
                    name,
                })
                .collect(),
            findings,
            visibility,
        }
    }

    async fn save_mod_log_settings(
        &self,
        user: UserRef,
        g: CommunityRef,
        f: ModLogSettingsForm,
    ) -> Result<ModLogSettingsView, ModLogSaveRefused> {
        let (Ok(g), Ok(user)) = (guild_id(&g), user_id(&user)) else {
            return Err(ModLogSaveRefused::NotLoaded);
        };
        use crack_core::modlog::{
            live::after_save, save_settings, settings::ModLogSettings, ModLogSaveError,
        };
        let channel = match f.channel.as_deref() {
            Some(raw) => Some(
                self.text_channel_of(g, raw)
                    .ok_or(ModLogSaveRefused::UnknownChannel)?,
            ),
            None => None,
        };
        let settings = ModLogSettings {
            guild: g,
            enabled: f.enabled,
            channel,
        };
        save_settings(&self.deps.data.rooms_data(), settings)
            .await
            .map_err(|e| match e {
                ModLogSaveError::Invalid(e) => ModLogSaveRefused::Invalid(e),
                ModLogSaveError::Db => ModLogSaveRefused::Failed,
                ModLogSaveError::NotLoaded => ModLogSaveRefused::NotLoaded,
            })?;
        after_save(&self.deps.data.rooms_data(), self.deps.http.clone(), g);
        tracing::info!(guild = %g, user = %user, "dashboard saved mod log settings");
        Ok(self.mod_log_settings(g.to_ref()).await)
    }

    async fn economy_settings(&self, g: CommunityRef) -> EconomySettingsView {
        let Ok(g) = guild_id(&g) else {
            return economy_settings_off();
        };
        use crack_core::economy::preflight::bot_perms;
        let stored = self.deps.data.economy.settings(g);
        let mut view = EconomySettingsView::from_settings(&stored, crack_core::COIN_BUILD);
        // Without the guild in the cache the view keeps its fallbacks: the
        // stored channels by id, and a report that can't tell.
        let Some(guild) = self.deps.cache.guild(g) else {
            return view;
        };
        let options = |kind: ChannelType| {
            let mut v: Vec<_> = guild
                .channels
                .iter()
                .filter(|c| c.base.kind == kind)
                .map(|c| (c.position, c.id, c.base.name.to_string()))
                .collect();
            v.sort();
            v.into_iter()
                .map(|(_, id, name)| ChannelOption {
                    id: id.to_string(),
                    name,
                })
                .collect::<Vec<_>>()
        };
        view.voice_channels = options(ChannelType::Voice);
        view.text_channels = options(ChannelType::Text);
        let name_of = |c: ChannelId| {
            guild
                .channels
                .get(&c)
                .map(|ch| ch.base.name.to_string())
                .unwrap_or_else(|| format!("channel {c}"))
        };
        let c = &stored.config;
        let channels: Vec<ChannelId> = c
            .jail_channel_id
            .into_iter()
            .chain(c.announce_channel_id)
            .collect();
        let perms = bot_perms(&self.deps.cache, g, &channels);
        (view.preflight, view.announce_findings) =
            settings::economy_preflight(c, perms.as_ref(), name_of);
        view
    }

    async fn save_economy_settings(
        &self,
        user: UserRef,
        g: CommunityRef,
        f: EconomySettingsForm,
    ) -> Result<EconomySettingsView, EconomySaveRefused> {
        let (Ok(g), Ok(user)) = (guild_id(&g), user_id(&user)) else {
            return Err(EconomySaveRefused::NotLoaded);
        };
        use crack_core::economy::{save_settings, EconomySaveError};
        // From the stored settings, so a price this build does not know survives.
        let settings =
            settings::apply_form(
                self.deps.data.economy.settings(g),
                f,
                |kind, raw| match kind {
                    settings::ChannelKind::Voice => self.voice_channel_of(g, raw),
                    settings::ChannelKind::Text => self.text_channel_of(g, raw),
                },
            )?;
        save_settings(&self.deps.data.economy_data(), settings)
            .await
            .map_err(|e| match e {
                EconomySaveError::Invalid(e) => EconomySaveRefused::Invalid(e),
                EconomySaveError::Db => EconomySaveRefused::Failed,
                EconomySaveError::NotLoaded => EconomySaveRefused::NotLoaded,
            })?;
        tracing::info!(guild = %g, user = %user, "dashboard saved economy settings");
        Ok(self.economy_settings(g.to_ref()).await)
    }

    // Needs a `LiveBackend`, which tests cannot build (no constructor without a live
    // `Data`, DB pool and Discord cache/HTTP), so the whole method, including its
    // filter, game exclusion and plan floor, is exempt from mutants.
    #[cfg_attr(test, mutants::skip)]
    async fn history(
        &self,
        g: CommunityRef,
        q: &HistoryQuery,
    ) -> Result<HistoryPage, HistoryError> {
        let Ok(g) = guild_id(&g) else {
            return Err(HistoryError::Failed);
        };
        let Some(pool) = self.deps.data.database_pool.clone() else {
            return Err(HistoryError::NoDatabase);
        };
        let filter = AuditFilter {
            user: q.user,
            action: q.action.map(|a| a.name()),
            source: q.source.map(|s| s.source().as_str()),
            since: q.since.map(|d| chrono::Utc::now() - d),
        };
        // Copy the start time out so the DashMap ref is dropped before any await.
        let running_since = self
            .deps
            .data
            .gp_games
            .get(&g)
            .map(|game| game.started_at)
            .and_then(|t| chrono::DateTime::from_timestamp(t, 0));
        // Read on every request: granting or removing premium applies at once.
        let floor = Plan::of(self.deps.data.get_premium(g).await).history_floor(chrono::Utc::now());
        // The game's rows are left out in SQL, so the cursors the page pages
        // by are ids it shows; `compose_page` checks again by the same rule.
        let rows = audit_page(
            &pool,
            g,
            &filter,
            q.cursor,
            history::fetch_limit(q.cursor),
            running_since,
        )
        .await
        .map_err(|e| {
            tracing::warn!("history: query failed in {g}: {e}");
            HistoryError::Failed
        })?;
        let names = self
            .names_for(g, &history::shown_members(&rows, running_since, floor))
            .await;
        Ok(history::compose_page(
            rows,
            q.cursor,
            running_since,
            floor,
            |u| names.get(&u).cloned(),
        ))
    }
}

/// Start the dashboard if its environment is complete. Missing keys are
/// logged by name and the dashboard stays off; nothing here can stop the bot.
pub fn spawn_if_configured(deps: WebDeps) -> Option<tokio::task::JoinHandle<()>> {
    let env = match WebEnv::from_lookup(|k| std::env::var(k).ok()) {
        Ok(env) => env,
        Err(missing) => {
            tracing::warn!(
                "web dashboard off; missing or unusable: {}",
                missing.join(", ")
            );
            return None;
        },
    };
    Some(tokio::spawn(async move {
        if let Err(e) = serve(env, deps).await {
            tracing::error!("web dashboard stopped: {e}");
        }
    }))
}

async fn serve(env: WebEnv, deps: WebDeps) -> std::io::Result<()> {
    let auth = Arc::new(
        catacombs::Auth::new(env.catacombs_config(), catacombs::MemoryStorage::new())
            .map_err(std::io::Error::other)?,
    );
    let backend = Arc::new(LiveBackend {
        deps,
        memo: MemberMemo::new(MEMBER_TTL),
        roles: access::RoleMemo::new(history::ROLE_TTL),
        names: history::NameMemo::new(history::NAME_TTL),
        owners: tokio::sync::OnceCell::new(),
    });
    let state = WebState {
        auth,
        hub: Hub::new(backend.clone(), TICK, LINGER),
        backend,
        origin: env.public_origin.clone().into(),
        limiter: Arc::new(limit::RateLimit::new(
            limit::CONTROLS_PER_WINDOW,
            limit::CONTROL_WINDOW,
        )),
        hits: Arc::new(hits::HitMemo::new(hits::HIT_TTL)),
        search_limiter: Arc::new(limit::RateLimit::new(
            limit::SEARCHES_PER_WINDOW,
            limit::SEARCH_WINDOW,
        )),
    };
    let listener = tokio::net::TcpListener::bind(&env.bind).await?;
    tracing::info!("web dashboard on {} for {}", env.bind, env.public_origin);
    axum::serve(listener, routes::router(state)).await
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn a_guild_that_is_not_a_snowflake_reads_as_never_configured() {
        let rooms = room_settings_off();
        assert!(!rooms.enabled && rooms.hub.is_none() && rooms.findings.is_none());
        assert_eq!(rooms.name_template, "{name}'s room");
        assert_eq!(rooms.empty_grace_secs, 30);
        assert!(rooms.voice_channels.is_empty());

        let modlog = mod_log_settings_off();
        assert!(!modlog.enabled && modlog.channel.is_none() && modlog.visibility.is_none());

        let economy = economy_settings_off();
        assert!(!economy.enabled);
        assert_eq!(economy.coin_build, crack_core::COIN_BUILD);
    }
}
