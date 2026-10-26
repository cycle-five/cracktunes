//! What the music player asks of the layer above it: refresh Discord after an
//! op (`settle`), and post a dashboard echo (`echo`). `CoreSurface` is the
//! Discord-backed implementation; tests use a recording one.

use crate::music::ops::{OpCx, Settle};
use crack_common::message::CrackedMessage;
use crack_common::render::RenderCx;
use crack_common::transport::Transport;
use serenity::all::{GenericChannelId, GuildId, MessageId};
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

#[serenity::async_trait]
pub trait Surface: Send + Sync {
    /// Refresh what Discord shows after an op. 🔑 Hold no Call lock: this
    /// locks it.
    async fn settle(
        &self,
        settle: Settle,
        cx: &OpCx,
        call: Option<&Arc<Mutex<Call>>>,
        anchor: Option<(GenericChannelId, MessageId)>,
    );

    /// Post a dashboard echo in the guild's bot channel, if its settings allow
    /// one, and say where it landed.
    async fn echo(
        &self,
        guild: GuildId,
        transport: &dyn Transport,
        msg: &CrackedMessage,
        cx: &RenderCx,
    ) -> Option<(GenericChannelId, MessageId)>;
}
