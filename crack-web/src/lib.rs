//! The Crack Tunes web dashboard (arc 1): view a guild's queue live, and
//! reorder it from the bot's voice channel. Runs inside the bot process; see
//! docs/superpowers/specs/2026-09-30-web-dashboard-queue-design.md.

pub mod access;
pub mod backend;
pub mod config;
pub mod page;
pub mod routes;
#[cfg(test)]
mod test_support;
pub mod view;
pub mod watch;

use crate::{
    access::{MemberMemo, Membership, Presence, MEMBER_TTL},
    backend::{Backend, GuildEntry, MoveRefused},
    config::WebEnv,
    routes::WebState,
    view::{view_from_state, QueueView},
    watch::{Hub, ViewSource, LINGER, TICK},
};
use crack_core::{music::remote, Data};
use serenity::all::{Cache, GuildId, Http, UserId};
use std::sync::Arc;
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
}

impl LiveBackend {
    fn member_name(&self, g: GuildId, u: UserId) -> Option<String> {
        let guild = self.deps.cache.guild(g)?;
        guild.members.get(&u).map(|m| m.display_name().to_owned())
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
        access::presence(&self.deps.cache, &self.deps.http, &self.memo, g, u, bot).await
    }

    async fn move_track(&self, g: GuildId, id: Uuid, to: usize) -> Result<usize, MoveRefused> {
        remote::move_by_id(self.deps.data.clone(), &self.deps.http, g, id, to).await
    }

    async fn guilds_for(&self, u: UserId) -> Vec<GuildEntry> {
        let mut out = Vec::new();
        for (g, channel) in remote::active_guilds(&self.deps.data).await {
            let p = access::presence(
                &self.deps.cache,
                &self.deps.http,
                &self.memo,
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
