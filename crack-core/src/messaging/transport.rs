//! The wire to Discord for channel messages: send, edit, delete, and what the
//! cache knows of a channel. Tests swap in `test_support::FakeTransport`.
use crate::http_utils::is_unknown_message;
use crate::messaging::render::Rendered;
use serenity::all::{Cache, GenericChannelId, GuildId, Http, MessageId};
use serenity::async_trait;
use std::sync::Arc;

/// Why a send, edit or delete did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Discord says the message is gone (deleted by hand or by `/clean`).
    UnknownMessage,
    /// Anything else, as text for the log.
    Other(String),
}

impl From<serenity::Error> for TransportError {
    fn from(err: serenity::Error) -> Self {
        if is_unknown_message(&err) {
            Self::UnknownMessage
        } else {
            Self::Other(err.to_string())
        }
    }
}

/// Everything the layers above need from Discord, so tests can run without it.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(
        &self,
        channel: GenericChannelId,
        out: Rendered,
    ) -> Result<MessageId, TransportError>;
    async fn edit(
        &self,
        channel: GenericChannelId,
        id: MessageId,
        out: Rendered,
    ) -> Result<(), TransportError>;
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError>;
    /// The newest message id the gateway has reported for `channel`, if the
    /// channel is cached.
    fn last_message_id(&self, guild: GuildId, channel: GenericChannelId) -> Option<MessageId>;
}

/// The real Discord behind [`Transport`].
pub struct DiscordTransport {
    pub http: Arc<Http>,
    pub cache: Arc<Cache>,
}

impl DiscordTransport {
    /// The transport a command or event handler already has the pieces for.
    #[must_use]
    pub fn of(ctx: &serenity::all::Context) -> Self {
        Self {
            http: ctx.http.clone(),
            cache: ctx.cache.clone(),
        }
    }
}

#[async_trait]
impl Transport for DiscordTransport {
    #[expect(
        clippy::disallowed_methods,
        reason = "messaging is where sends are made"
    )]
    async fn send(
        &self,
        channel: GenericChannelId,
        out: Rendered,
    ) -> Result<MessageId, TransportError> {
        Ok(channel.send_message(&self.http, out.to_message()).await?.id)
    }

    async fn edit(
        &self,
        channel: GenericChannelId,
        id: MessageId,
        out: Rendered,
    ) -> Result<(), TransportError> {
        #[expect(
            clippy::disallowed_methods,
            reason = "messaging is where sends are made"
        )]
        channel.edit_message(&self.http, id, out.to_edit()).await?;
        Ok(())
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "messaging is where sends are made"
    )]
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError> {
        Ok(channel.delete_message(&self.http, id, None).await?)
    }

    /// serenity sets `last_message_id` on every message-create for guild
    /// channels and threads (`cache/event.rs`). None when the guild or channel
    /// is not cached, which `placement` treats as "moved".
    fn last_message_id(&self, guild: GuildId, channel: GenericChannelId) -> Option<MessageId> {
        let guild = self.cache.guild(guild)?;
        let (channel_id, thread_id) = channel.split();
        match guild.channels.get(&channel_id) {
            Some(guild_channel) => guild_channel.base.last_message_id,
            None => guild
                .threads
                .get(&thread_id)
                .and_then(|thread| thread.base.last_message_id),
        }
    }
}
