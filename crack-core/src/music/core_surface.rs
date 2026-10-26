//! The Discord-backed [`Surface`]: settles through the status and queue
//! messages, and echoes through the courier.

use crate::handlers::track_end::update_queue_messages;
use crate::messaging::courier::{self, Destination};
use crate::messaging::message::CrackedMessage;
use crate::messaging::render::RenderCx;
use crate::messaging::status;
use crate::messaging::transport::Transport;
use crate::music::ops::{OpCx, Settle};
use crate::music::surface::Surface;
use crate::Data;
use serenity::all::{GenericChannelId, GuildId, MessageId};
use songbird::Call;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct CoreSurface {
    pub data: Arc<Data>,
}

#[serenity::async_trait]
impl Surface for CoreSurface {
    async fn settle(
        &self,
        settle: Settle,
        cx: &OpCx,
        call: Option<&Arc<Mutex<Call>>>,
        anchor: Option<(GenericChannelId, MessageId)>,
    ) {
        match (settle, call) {
            (Settle::QueueMessages, Some(call)) => {
                let queue = call.lock().await.queue().current_queue();
                update_queue_messages(&cx.transport(), self.data.clone(), &queue, cx.guild_id)
                    .await;
            },
            (Settle::NowPlaying, Some(call)) => {
                let playing = call.lock().await.queue().current().is_some();
                if playing {
                    status::show_now_playing_after(
                        &self.data,
                        cx.http.clone(),
                        cx.cache.clone(),
                        cx.guild_id,
                        call,
                        anchor,
                    )
                    .await;
                }
            },
            (Settle::Finished, _) => {
                status::show_finished(&self.data, cx.http.clone(), cx.cache.clone(), cx.guild_id)
                    .await;
            },
            _ => {},
        }
    }

    async fn echo(
        &self,
        guild: GuildId,
        transport: &dyn Transport,
        msg: &CrackedMessage,
        cx: &RenderCx,
    ) -> Option<(GenericChannelId, MessageId)> {
        courier::post(&self.data, transport, Destination::Echo(guild), msg, cx).await
    }
}
