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

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Data, DataInner};
    const G: serenity::all::GuildId = serenity::all::GuildId::new(1);

    /// Review Focus 4: the echo the dashboard posts goes through the surface,
    /// and the real surface honours a guild's "control echoes off".
    #[tokio::test]
    async fn remote_echo_respects_control_echoes_off() {
        use crate::guild::settings::GuildSettings;
        use crack_common::test_support::FakeTransport;
        let d = Data(Arc::new(DataInner::default()));
        crate::messaging::status::note_command_channel(
            &d,
            G,
            serenity::all::GenericChannelId::new(10),
        )
        .await;
        let mut settings = GuildSettings::new(crack_types::discord::ToRef::to_ref(&G), None, None);
        settings.control_echoes = false;
        d.guild_settings_map.write().await.insert(G, settings);
        let t = FakeTransport::default();
        let at = d
            .music_data()
            .surface
            .echo(G, &t, &CrackedMessage::Clear, &RenderCx::now())
            .await;
        assert_eq!(at, None);
        assert!(t.ops().is_empty(), "{:?}", t.ops());
    }
}
