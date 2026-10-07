//! Stand-in Discord for messaging tests: everything sent is recorded.
use super::courier::ReplySink;
use super::render::{description, Rendered};
use super::transport::{Transport, TransportError};
use crate::errors::CrackedError;
use async_trait::async_trait;
use serenity::all::{GenericChannelId, GuildId, MessageId};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Send(u64),
    Edit(u64, u64),
    Delete(u64, u64),
}

/// Records every call, keeps every rendered message, answers from what the
/// test set.
#[derive(Default)]
pub struct FakeTransport {
    pub last: Mutex<Option<MessageId>>,
    pub edit_error: Mutex<Option<TransportError>>,
    pub delete_error: Mutex<Option<TransportError>>,
    pub send_error: Mutex<Option<TransportError>>,
    pub ops: Mutex<Vec<Op>>,
    pub sent: Mutex<Vec<Rendered>>,
    next: AtomicU64,
}

impl FakeTransport {
    #[must_use]
    pub fn with_last(self, last: u64) -> Self {
        *self.last.lock().unwrap() = Some(MessageId::new(last));
        self
    }
    pub fn ops(&self) -> Vec<Op> {
        self.ops.lock().unwrap().clone()
    }
    /// The embed descriptions (else content) of everything sent or edited, in order.
    pub fn texts(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|r| {
                description(r)
                    .or_else(|| r.content.clone())
                    .unwrap_or_default()
            })
            .collect()
    }
}

#[async_trait]
impl Transport for FakeTransport {
    async fn send(
        &self,
        channel: GenericChannelId,
        out: Rendered,
    ) -> Result<MessageId, TransportError> {
        self.ops.lock().unwrap().push(Op::Send(channel.get()));
        self.sent.lock().unwrap().push(out);
        if let Some(err) = self.send_error.lock().unwrap().clone() {
            return Err(err);
        }
        Ok(MessageId::new(
            1000 + self.next.fetch_add(1, Ordering::SeqCst),
        ))
    }
    async fn edit(
        &self,
        channel: GenericChannelId,
        id: MessageId,
        out: Rendered,
    ) -> Result<(), TransportError> {
        self.ops
            .lock()
            .unwrap()
            .push(Op::Edit(channel.get(), id.get()));
        self.sent.lock().unwrap().push(out);
        match self.edit_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    async fn delete(&self, channel: GenericChannelId, id: MessageId) -> Result<(), TransportError> {
        self.ops
            .lock()
            .unwrap()
            .push(Op::Delete(channel.get(), id.get()));
        match self.delete_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    fn last_message_id(&self, _guild: GuildId, _channel: GenericChannelId) -> Option<MessageId> {
        *self.last.lock().unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyOp {
    Send { ephemeral: bool, text: String },
    EditHandle { handle: u64, text: String },
}

/// A stand-in for poise's reply path. Handles are numbered from 1.
#[derive(Default)]
pub struct FakeReplies {
    pub ops: Mutex<Vec<ReplyOp>>,
    pub sent: Mutex<Vec<Rendered>>,
    next: AtomicU64,
}

impl FakeReplies {
    pub fn ops(&self) -> Vec<ReplyOp> {
        self.ops.lock().unwrap().clone()
    }
}

fn text_of(r: &Rendered) -> String {
    description(r)
        .or_else(|| r.content.clone())
        .unwrap_or_default()
}

#[async_trait]
impl ReplySink for FakeReplies {
    type Handle = u64;
    async fn send(&self, out: Rendered, ephemeral: bool) -> Result<u64, CrackedError> {
        self.ops.lock().unwrap().push(ReplyOp::Send {
            ephemeral,
            text: text_of(&out),
        });
        self.sent.lock().unwrap().push(out);
        Ok(1 + self.next.fetch_add(1, Ordering::SeqCst))
    }
    async fn edit(&self, handle: &u64, out: Rendered) -> Result<(), CrackedError> {
        self.ops.lock().unwrap().push(ReplyOp::EditHandle {
            handle: *handle,
            text: text_of(&out),
        });
        self.sent.lock().unwrap().push(out);
        Ok(())
    }
    async fn locate(&self, handle: &u64) -> Option<(GenericChannelId, MessageId)> {
        Some((GenericChannelId::new(5), MessageId::new(*handle)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressOp {
    Acknowledge,
    Followup { ephemeral: bool, text: String },
}

/// A stand-in for one button press.
#[derive(Default)]
pub struct FakePress {
    pub ops: Mutex<Vec<PressOp>>,
    pub ack_error: Mutex<Option<TransportError>>,
}

impl FakePress {
    pub fn ops(&self) -> Vec<PressOp> {
        self.ops.lock().unwrap().clone()
    }
}

#[async_trait]
impl super::transport::Press for FakePress {
    async fn acknowledge(&self) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Acknowledge);
        match self.ack_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    async fn followup(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Followup {
            ephemeral,
            text: text_of(&out),
        });
        Ok(())
    }
}
