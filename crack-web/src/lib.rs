//! The Crack Tunes web dashboard (arc 1): view a guild's queue live, and
//! reorder it from the bot's voice channel. Runs inside the bot process; see
//! docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md.

pub mod access;
pub mod backend;
pub mod config;
pub mod history;
pub mod page;
pub mod routes;
#[cfg(test)]
mod test_support;
pub mod view;
pub mod watch;

use crate::{
    access::{HistoryAccess, MemberMemo, Membership, Presence, MEMBER_TTL},
    backend::{Backend, GuildEntry, HistoryError, MoveRefused},
    config::WebEnv,
    history::{HistoryPage, HistoryQuery},
    routes::WebState,
    view::{view_from_state, QueueView},
    watch::{Hub, ViewSource, LINGER, TICK},
};
use crack_core::{
    db::queue_audit::{audit_page, AuditFilter},
    music::remote,
    Data,
};
use serenity::all::{Cache, GuildId, Http, Permissions, RoleId, UserId};
use std::{collections::HashMap, sync::Arc};
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
}

impl LiveBackend {
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

impl ViewSource for LiveBackend {
    async fn view(&self, g: GuildId) -> QueueView {
        let state = remote::queue_state(&self.deps.data, g).await;
        view_from_state(state, |u| self.member_name(g, u))
    }
}

impl Backend for LiveBackend {
    async fn presence(&self, g: GuildId, u: UserId) -> Presence {
        let bot = remote::bot_channel(&self.deps.data, g).await;
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
        user: UserId,
        g: GuildId,
        id: Uuid,
        to: usize,
    ) -> Result<usize, MoveRefused> {
        remote::move_by_id(
            self.deps.data.clone(),
            self.deps.http.clone(),
            g,
            user,
            id,
            to,
        )
        .await
    }

    async fn guilds_for(&self, u: UserId) -> Vec<GuildEntry> {
        let mut out = Vec::new();
        for (g, channel) in remote::active_guilds(&self.deps.data).await {
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
                id: g,
                name,
                channel,
            });
        }
        out
    }

    fn guild_name(&self, g: GuildId) -> Option<String> {
        self.deps.cache.guild(g).map(|guild| guild.name.to_string())
    }

    async fn history_access(&self, g: GuildId, u: UserId) -> HistoryAccess {
        let membership = self.presence(g, u).await.membership;
        if membership != Membership::Member {
            return access::decide_history(membership, None);
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
        access::decide_history(membership, manages)
    }

    async fn history(&self, g: GuildId, q: &HistoryQuery) -> Result<HistoryPage, HistoryError> {
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
            .names_for(g, &history::shown_members(&rows, running_since))
            .await;
        Ok(history::compose_page(rows, q.cursor, running_since, |u| {
            names.get(&u).cloned()
        }))
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
    let auth = Arc::new(catacombs::AppState::new(
        env.catacombs_config(),
        catacombs::MemoryStorage::new(),
    ));
    let backend = Arc::new(LiveBackend {
        deps,
        memo: MemberMemo::new(MEMBER_TTL),
        roles: access::RoleMemo::new(history::ROLE_TTL),
        names: history::NameMemo::new(history::NAME_TTL),
    });
    let state = WebState {
        auth,
        hub: Hub::new(backend.clone(), TICK, LINGER),
        backend,
        origin: env.public_origin.clone().into(),
    };
    let listener = tokio::net::TcpListener::bind(&env.bind).await?;
    tracing::info!("web dashboard on {} for {}", env.bind, env.public_origin);
    axum::serve(listener, routes::router(state)).await
}
