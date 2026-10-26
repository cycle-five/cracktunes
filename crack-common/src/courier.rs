//! Delivery that needs no command context: sends through a [`Transport`] and
//! answers through a [`Press`], all rendered by [`render`]. crack-core's
//! `courier` adds the command replies and `post`, which read `Data`.

use crate::message::CrackedMessage;
use crate::render::{render, RenderCx, Rendered};
use crate::transport::{Press, Transport, TransportError};
use serenity::all::{GenericChannelId, MessageId};

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

/// `post`'s channel destination: send `out` to `channel`. Best effort: a
/// failure is logged and swallowed, and the result says where it landed, if
/// anywhere.
pub async fn post_to_channel(
    transport: &dyn Transport,
    channel: GenericChannelId,
    out: Rendered,
) -> Option<(GenericChannelId, MessageId)> {
    match transport.send(channel, out).await {
        Ok(id) => Some((channel, id)),
        Err(err) => {
            tracing::warn!("post to {channel} failed: {err:?}");
            None
        },
    }
}

/// Tell Discord a button press arrived. Best effort: `false`, logged, when it
/// failed (the press is still handled; Discord shows its own failure notice).
pub async fn acknowledge(press: &dyn Press) -> bool {
    match press.acknowledge().await {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!("acknowledging a button press failed: {err:?}");
            false
        },
    }
}

/// Answer the presser only. Best effort: a failure is logged.
pub async fn answer_privately(press: &dyn Press, msg: &CrackedMessage, cx: &RenderCx) {
    if let Err(err) = press.followup(render(msg, cx), true).await {
        tracing::warn!("answering a button press failed: {err:?}");
    }
}

/// Answer a press with its one response, to the presser only when
/// `ephemeral`. Fallible: the caller decides whether a lost answer matters.
pub async fn respond(
    press: &dyn Press,
    msg: &CrackedMessage,
    cx: &RenderCx,
    ephemeral: bool,
) -> Result<(), TransportError> {
    press.respond(render(msg, cx), ephemeral).await
}

/// Answer a press by redrawing the message it came from. Fallible.
pub async fn update(
    press: &dyn Press,
    msg: &CrackedMessage,
    cx: &RenderCx,
) -> Result<(), TransportError> {
    press.update(render(msg, cx)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{FakePress, FakeTransport, Op, PressOp};

    fn button(id: &str) -> crack_model::Button {
        crack_model::Button {
            id: id.into(),
            label: None,
            emoji: None,
            style: crack_model::ButtonStyle::Primary,
            disabled: false,
        }
    }

    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 0,
            embed_links: true,
        }
    }

    #[tokio::test]
    async fn post_to_channel_says_where_it_landed() {
        let t = FakeTransport::default();
        let ch = GenericChannelId::new(7);
        let landed = post_to_channel(&t, ch, Rendered::text("hi")).await;
        assert_eq!(landed.map(|(c, _)| c), Some(ch));
        assert_eq!(t.ops(), [Op::Send(7)]);
    }

    #[tokio::test]
    async fn post_to_channel_swallows_a_failed_send() {
        let t = FakeTransport::default();
        *t.send_error.lock().unwrap() = Some(TransportError::Other("down".into()));
        let landed = post_to_channel(&t, GenericChannelId::new(7), Rendered::text("hi")).await;
        assert_eq!(landed, None);
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

    #[tokio::test]
    async fn a_press_is_acknowledged_then_answered_privately() {
        let p = FakePress::default();
        assert!(acknowledge(&p).await);
        answer_privately(
            &p,
            &CrackedMessage::from(crate::errors::CrackedError::NothingPlaying),
            &cx(),
        )
        .await;
        assert_eq!(
            p.ops(),
            vec![
                PressOp::Acknowledge,
                PressOp::Followup {
                    ephemeral: true,
                    text: "🔈 Nothing is playing!".into()
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_acknowledge_is_reported_not_raised() {
        let p = FakePress::default();
        *p.ack_error.lock().unwrap() = Some(TransportError::Other("Unknown interaction".into()));
        assert!(!acknowledge(&p).await);
    }

    #[tokio::test]
    async fn a_press_answered_now_is_one_private_response() {
        let press = FakePress::default();
        respond(&press, &CrackedMessage::Other("ok".into()), &cx(), true)
            .await
            .unwrap();
        assert_eq!(
            press.ops(),
            vec![PressOp::Respond {
                ephemeral: true,
                text: "ok".into()
            }]
        );
    }

    #[tokio::test]
    async fn an_update_redraws_the_message_with_its_rows() {
        let press = FakePress::default();
        let msg = CrackedMessage::Other("page".into());
        // `render` gives an `Other` no components; give it two rows to see them counted.
        let out = render(&msg, &cx()).with_rows(vec![
            crack_model::Row::Buttons(vec![button("a")]),
            crack_model::Row::Buttons(vec![button("b")]),
        ]);
        press.update(out).await.unwrap();
        update(&press, &msg, &cx()).await.unwrap();
        assert_eq!(
            press.ops(),
            vec![
                PressOp::Update {
                    text: "page".into(),
                    rows: 2
                },
                PressOp::Update {
                    text: "page".into(),
                    rows: 0
                },
            ]
        );
    }

    #[tokio::test]
    async fn clearing_components_touches_only_the_components() {
        let t = FakeTransport::default();
        t.clear_components(GenericChannelId::new(5), MessageId::new(9))
            .await
            .unwrap();
        assert_eq!(t.ops(), vec![Op::ClearComponents(5, 9)]);
        assert!(t.sent.lock().unwrap().is_empty(), "nothing was re-rendered");
    }

    #[tokio::test]
    async fn queued_send_failures_fail_that_many_sends_then_let_them_through() {
        let t = FakeTransport::default();
        t.send_failures
            .lock()
            .unwrap()
            .push_back(TransportError::Other("503".into()));
        let first = t.send(GenericChannelId::new(5), Rendered::text("a")).await;
        let second = t.send(GenericChannelId::new(5), Rendered::text("a")).await;
        assert_eq!(first, Err(TransportError::Other("503".into())));
        assert_eq!(second, Ok(MessageId::new(1000)));
    }

    #[test]
    fn a_transport_error_reads_as_text() {
        assert_eq!(
            TransportError::UnknownMessage.to_string(),
            "unknown message"
        );
        assert_eq!(TransportError::Other("boom".into()).to_string(), "boom");
    }
}
