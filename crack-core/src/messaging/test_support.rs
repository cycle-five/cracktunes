//! Stand-in Discord for messaging tests: everything sent is recorded.
pub use crack_common::test_support::*;

use super::courier::ReplySink;
use super::render::Rendered;
use crate::errors::CrackedError;
use serenity::all::{GenericChannelId, MessageId};
use serenity::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyOp {
    Send { ephemeral: bool, text: String },
    EditHandle { handle: u64, text: String },
    Retire { handle: u64, text: String },
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
    async fn retire(&self, handle: &u64, out: Rendered) -> Result<(), CrackedError> {
        self.ops.lock().unwrap().push(ReplyOp::Retire {
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
