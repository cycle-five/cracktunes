//! Delivery: replies through a [`ReplySink`], background posts through a
//! [`Transport`], all rendered by [`render`]. Spec: the messaging-layer design.
use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::messaging::render::{render, RenderCx, Rendered};
use crate::messaging::status::{self, Phase};
use crate::messaging::transport::{Transport, TransportError};
use crate::Data;
use serenity::all::{GenericChannelId, GuildId, MessageId};
use serenity::async_trait;

#[async_trait]
pub trait ReplySink: Send + Sync {
    type Handle: Send + Sync;
    async fn send(&self, out: Rendered, ephemeral: bool) -> Result<Self::Handle, CrackedError>;
    async fn edit(&self, handle: &Self::Handle, out: Rendered) -> Result<(), CrackedError>;
    /// The reply as a channel message, `None` for an ephemeral one or when
    /// it cannot be read.
    async fn locate(&self, handle: &Self::Handle) -> Option<(GenericChannelId, MessageId)>;
}

/// [`ReplySink`] over poise: the command's own reply path.
pub struct PoiseReplies<'ctx>(pub crate::Context<'ctx>);

#[async_trait]
impl<'ctx> ReplySink for PoiseReplies<'ctx> {
    type Handle = poise::ReplyHandle<'ctx>;

    /// The ephemeral flag is set at send time: Discord cannot change it later.
    #[expect(
        clippy::disallowed_methods,
        reason = "messaging is where sends are made"
    )]
    async fn send(&self, out: Rendered, ephemeral: bool) -> Result<Self::Handle, CrackedError> {
        self.0
            .send(out.to_reply(ephemeral))
            .await
            .map_err(Into::into)
    }

    /// #494: `ReplyHandle::edit` routes the response and its followups to the
    /// right message; an edit never goes to `@original` by guesswork.
    async fn edit(&self, handle: &Self::Handle, out: Rendered) -> Result<(), CrackedError> {
        edit_poise(self.0, handle, out).await
    }

    /// `None`, with a warning, when poise cannot produce the message (a slash
    /// command's initial response is fetched over HTTP).
    async fn locate(&self, handle: &Self::Handle) -> Option<(GenericChannelId, MessageId)> {
        match handle.message().await {
            Ok(m) => Some((m.channel_id, m.id)),
            Err(err) => {
                tracing::warn!(
                    "status: could not read the reply to place the status below it: {err}"
                );
                None
            },
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "messaging is where sends are made"
)]
async fn edit_poise<'ctx>(
    ctx: crate::Context<'ctx>,
    handle: &poise::ReplyHandle<'ctx>,
    out: Rendered,
) -> Result<(), CrackedError> {
    let (reply, dropped_components) = out.to_reply_edit();
    if dropped_components {
        tracing::warn!("a reply edit cannot carry components (poise ties the builder to the context's lifetime); sent without");
    }
    handle.edit(ctx, reply).await.map_err(Into::into)
}

pub async fn reply_on<S: ReplySink>(
    sink: &S,
    msg: &CrackedMessage,
    cx: &RenderCx,
    ephemeral: bool,
) -> Result<S::Handle, CrackedError> {
    sink.send(render(msg, cx), ephemeral).await
}

pub async fn edit_reply_on<S: ReplySink>(
    sink: &S,
    handle: &S::Handle,
    msg: &CrackedMessage,
    cx: &RenderCx,
) -> Result<(), CrackedError> {
    sink.edit(handle, render(msg, cx)).await
}

pub async fn locate_on<S: ReplySink>(
    sink: &S,
    handle: &S::Handle,
) -> Option<(GenericChannelId, MessageId)> {
    sink.locate(handle).await
}

/// Where a reply the caller just posted landed, as the `after` floor for the
/// status. Only for a *visible* reply.
pub async fn locate<'ctx>(
    ctx: crate::Context<'ctx>,
    handle: &poise::ReplyHandle<'ctx>,
) -> Option<(GenericChannelId, MessageId)> {
    locate_on(&PoiseReplies(ctx), handle).await
}

pub async fn reply<'ctx>(
    ctx: crate::Context<'ctx>,
    msg: CrackedMessage,
) -> Result<poise::ReplyHandle<'ctx>, CrackedError> {
    reply_as(ctx, msg, false).await
}

pub async fn reply_as<'ctx>(
    ctx: crate::Context<'ctx>,
    msg: CrackedMessage,
    ephemeral: bool,
) -> Result<poise::ReplyHandle<'ctx>, CrackedError> {
    reply_on(&PoiseReplies(ctx), &msg, &RenderCx::now(), ephemeral).await
}

pub async fn edit_reply<'ctx>(
    ctx: crate::Context<'ctx>,
    handle: &poise::ReplyHandle<'ctx>,
    msg: CrackedMessage,
) -> Result<(), CrackedError> {
    edit_reply_on(&PoiseReplies(ctx), handle, &msg, &RenderCx::now()).await
}

/// A reply that is already rendered (the degraded EMBED_LINKS case builds
/// content + embed together).
pub async fn reply_rendered<'ctx>(
    ctx: crate::Context<'ctx>,
    out: Rendered,
    ephemeral: bool,
) -> Result<poise::ReplyHandle<'ctx>, CrackedError> {
    PoiseReplies(ctx).send(out, ephemeral).await
}

pub async fn edit_rendered<'ctx>(
    ctx: crate::Context<'ctx>,
    handle: &poise::ReplyHandle<'ctx>,
    out: Rendered,
) -> Result<(), CrackedError> {
    PoiseReplies(ctx).edit(handle, out).await
}

pub enum Destination {
    Channel(GenericChannelId),
    /// The floating status message; `after` is a visible reply it must land below.
    Status {
        guild: GuildId,
        after: Option<(GenericChannelId, MessageId)>,
    },
    /// Where Status would land; the tracked status message is left alone.
    Echo(GuildId),
}

/// A fallible send for when the message *is* the command's product (`/grab`'s
/// DM): nothing is rendered or swallowed here, the caller passes the
/// `Rendered` and gets the transport's error back.
pub async fn post_message(
    transport: &dyn Transport,
    channel: GenericChannelId,
    out: &Rendered,
) -> Result<MessageId, TransportError> {
    transport.send(channel, out.clone()).await
}

/// Edit a channel message the caller holds by `(channel, id)`, such as
/// `/play`'s playlist progress line. Fallible: the caller decides whether a
/// failed edit matters.
pub async fn edit_message(
    transport: &dyn Transport,
    channel: GenericChannelId,
    id: MessageId,
    msg: &CrackedMessage,
) -> Result<(), TransportError> {
    transport
        .edit(channel, id, render(msg, &RenderCx::now()))
        .await
}

/// Edit a channel message with an already rendered body. Unlike a reply edit,
/// a channel-message edit keeps its components (queue pages keep their nav
/// buttons).
pub async fn edit_rendered_message(
    transport: &dyn Transport,
    channel: GenericChannelId,
    id: MessageId,
    out: Rendered,
) -> Result<(), TransportError> {
    transport.edit(channel, id, out).await
}

/// Deliver `msg` to `dest`. Best effort: failures are logged and swallowed,
/// and the result says where it landed, if anywhere.
pub async fn post(
    data: &Data,
    transport: &dyn Transport,
    dest: Destination,
    msg: &CrackedMessage,
    cx: &RenderCx,
) -> Option<(GenericChannelId, MessageId)> {
    let out = render(msg, cx);
    match dest {
        Destination::Channel(channel) => match transport.send(channel, out).await {
            Ok(id) => Some((channel, id)),
            Err(err) => {
                tracing::warn!("post to {channel} failed: {err:?}");
                None
            },
        },
        Destination::Status { guild, after } => {
            let phase = match msg {
                CrackedMessage::Finished => Phase::Finished,
                _ => Phase::Playing,
            };
            status::update_after(data, transport, guild, out, phase, after)
                .await
                .map(|shown| (shown.channel, shown.id))
        },
        Destination::Echo(guild) => status::announce(data, transport, guild, out).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::format::TrackLabel;
    use crate::messaging::status;
    use crate::messaging::test_support::{FakeReplies, FakeTransport, Op, ReplyOp};
    use crate::messaging::transport::TransportError;
    use crate::{Data, DataInner};
    use std::sync::Arc;

    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 0,
            embed_links: true,
        }
    }

    #[tokio::test]
    async fn a_reply_is_rendered_and_sent_with_its_privacy() {
        let sink = FakeReplies::default();
        reply_on(&sink, &CrackedMessage::Skip, &cx(), true)
            .await
            .unwrap();
        assert_eq!(
            sink.ops(),
            vec![ReplyOp::Send {
                ephemeral: true,
                text: "⏭️ Skipped!".into()
            }]
        );
    }

    /// #494: an edit goes to the handle it was given, never `@original`.
    #[tokio::test]
    async fn an_edit_targets_the_handle_it_was_given() {
        let sink = FakeReplies::default();
        let first = reply_on(&sink, &CrackedMessage::Search, &cx(), false)
            .await
            .unwrap();
        let second = reply_on(&sink, &CrackedMessage::Search, &cx(), false)
            .await
            .unwrap();
        edit_reply_on(
            &sink,
            &second,
            &CrackedMessage::SkipTo(TrackLabel::default()),
            &cx(),
        )
        .await
        .unwrap();
        assert_eq!(first, 1);
        assert!(matches!(
            sink.ops()[2],
            ReplyOp::EditHandle { handle: 2, .. }
        ));
    }

    /// The status floor: a visible reply's place is what the next status
    /// update lands below (`status::update_after`'s `after`).
    #[tokio::test]
    async fn a_visible_reply_says_where_it_landed() {
        let sink = FakeReplies::default();
        let h = reply_on(&sink, &CrackedMessage::Skip, &cx(), false)
            .await
            .unwrap();
        assert_eq!(
            locate_on(&sink, &h).await,
            Some((GenericChannelId::new(5), MessageId::new(1)))
        );
    }

    #[tokio::test]
    async fn posting_to_a_channel_sends_once_and_says_where() {
        let data = Data(Arc::new(DataInner::default()));
        let t = FakeTransport::default();
        let at = post(
            &data,
            &t,
            Destination::Channel(GenericChannelId::new(7)),
            &CrackedMessage::Clear,
            &cx(),
        )
        .await;
        assert_eq!(t.ops(), vec![Op::Send(7)]);
        assert_eq!(at, Some((GenericChannelId::new(7), MessageId::new(1000))));
    }

    /// Covers `/grab`'s DM: the glue needs a live context, this is the
    /// fallible seam it relies on.
    #[tokio::test]
    async fn post_message_returns_the_transport_failure() {
        let t = FakeTransport::default();
        let out = render(&CrackedMessage::Clear, &cx());
        let ok = post_message(&t, GenericChannelId::new(7), &out).await;
        assert_eq!(ok, Ok(MessageId::new(1000)));
        assert_eq!(t.ops(), vec![Op::Send(7)]);
        *t.send_error.lock().unwrap() = Some(TransportError::Other("Cannot send".into()));
        let err = post_message(&t, GenericChannelId::new(7), &out).await;
        assert_eq!(err, Err(TransportError::Other("Cannot send".into())));
    }

    /// The playlist progress line edits the message it was given, rendered.
    #[tokio::test]
    async fn edit_message_edits_that_message() {
        let t = FakeTransport::default();
        edit_message(
            &t,
            GenericChannelId::new(7),
            MessageId::new(42),
            &CrackedMessage::Other("Queuing playlist... 24/100".into()),
        )
        .await
        .unwrap();
        assert_eq!(t.ops(), vec![Op::Edit(7, 42)]);
        assert_eq!(t.texts(), vec!["Queuing playlist... 24/100".to_owned()]);
        *t.edit_error.lock().unwrap() = Some(TransportError::UnknownMessage);
        let err = edit_message(
            &t,
            GenericChannelId::new(7),
            MessageId::new(42),
            &CrackedMessage::Clear,
        )
        .await;
        assert_eq!(err, Err(TransportError::UnknownMessage));
    }

    /// A queue page refresh edits in place and keeps its nav buttons.
    #[tokio::test]
    async fn edit_rendered_message_edits_that_message_with_its_components() {
        let t = FakeTransport::default();
        let out = Rendered::embed(serenity::all::CreateEmbed::new().description("page"))
            .with_components(crate::messaging::interface::create_nav_btns(0, 2));
        edit_rendered_message(&t, GenericChannelId::new(7), MessageId::new(42), out)
            .await
            .unwrap();
        assert_eq!(t.ops(), vec![Op::Edit(7, 42)]);
        let sent = t.sent.lock().unwrap();
        assert_eq!(sent.last().map(|r| r.components.len()), Some(1));
    }

    #[tokio::test]
    async fn a_failed_channel_post_is_swallowed() {
        let data = Data(Arc::new(DataInner::default()));
        let t = FakeTransport::default();
        *t.send_error.lock().unwrap() = Some(TransportError::Other("Missing Access".into()));
        let at = post(
            &data,
            &t,
            Destination::Channel(GenericChannelId::new(7)),
            &CrackedMessage::Clear,
            &cx(),
        )
        .await;
        assert_eq!(at, None);
    }

    #[tokio::test]
    async fn the_status_destination_tracks_the_message_and_finishing_marks_it() {
        let data = Data(Arc::new(DataInner::default()));
        let guild = GuildId::new(1);
        status::note_command_channel(&data, guild, GenericChannelId::new(10)).await;
        let t = FakeTransport::default().with_last(1000);
        let dest = || Destination::Status { guild, after: None };
        let at = post(&data, &t, dest(), &CrackedMessage::Clear, &cx()).await;
        assert_eq!(at, Some((GenericChannelId::new(10), MessageId::new(1000))));
        let at = post(&data, &t, dest(), &CrackedMessage::Finished, &cx()).await;
        assert_eq!(at, Some((GenericChannelId::new(10), MessageId::new(1000))));
        assert_eq!(t.ops(), vec![Op::Send(10), Op::Edit(10, 1000)]);
        let slot = data.status_slot(guild);
        let phase = slot.lock().await.message.map(|m| m.phase);
        assert_eq!(phase, Some(Phase::Finished));
    }

    #[tokio::test]
    async fn an_echo_lands_where_the_status_would_and_leaves_it_alone() {
        let data = Data(Arc::new(DataInner::default()));
        let guild = GuildId::new(1);
        status::note_command_channel(&data, guild, GenericChannelId::new(10)).await;
        let t = FakeTransport::default();
        let at = post(
            &data,
            &t,
            Destination::Echo(guild),
            &CrackedMessage::Clear,
            &cx(),
        )
        .await;
        assert_eq!(at, Some((GenericChannelId::new(10), MessageId::new(1000))));
        assert!(data.status_slot(guild).lock().await.message.is_none());
    }
}
