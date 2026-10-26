//! Stand-in Discord for messaging tests: everything sent is recorded.
use crate::render::{description, Rendered};
use crate::transport::{Press, Transport, TransportError};
use serenity::all::{GenericChannelId, GuildId, MessageId};
use serenity::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// What a test reads of a sent message: its embed description, else its content.
pub fn text_of(r: &Rendered) -> String {
    description(r)
        .or_else(|| r.content.clone())
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Send(u64),
    Edit(u64, u64),
    Delete(u64, u64),
    ClearComponents(u64, u64),
}

/// Records every call, keeps every rendered message, answers from what the
/// test set.
#[derive(Default)]
pub struct FakeTransport {
    pub last: Mutex<Option<MessageId>>,
    pub edit_error: Mutex<Option<TransportError>>,
    pub delete_error: Mutex<Option<TransportError>>,
    pub send_error: Mutex<Option<TransportError>>,
    /// Each `send` pops one; when empty it falls back to `send_error`.
    pub send_failures: Mutex<std::collections::VecDeque<TransportError>>,
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
        if let Some(err) = self.send_failures.lock().unwrap().pop_front() {
            return Err(err);
        }
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
    async fn clear_components(
        &self,
        channel: GenericChannelId,
        id: MessageId,
    ) -> Result<(), TransportError> {
        self.ops
            .lock()
            .unwrap()
            .push(Op::ClearComponents(channel.get(), id.get()));
        match self.edit_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    fn last_message_id(&self, _guild: GuildId, _channel: GenericChannelId) -> Option<MessageId> {
        *self.last.lock().unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressOp {
    Acknowledge,
    Followup {
        ephemeral: bool,
        text: String,
    },
    Respond {
        ephemeral: bool,
        text: String,
    },
    Update {
        text: String,
        rows: usize,
    },
    /// A modal opened, by its custom id.
    Modal(String),
}

/// A stand-in for one button press.
#[derive(Default)]
pub struct FakePress {
    pub ops: Mutex<Vec<PressOp>>,
    pub ack_error: Mutex<Option<TransportError>>,
    /// Every modal opened, whole, for inspection.
    pub modals: Mutex<Vec<serenity::all::CreateModal<'static>>>,
    /// What `open_modal` answers with, after recording the modal.
    pub modal_error: Mutex<Option<TransportError>>,
}

impl FakePress {
    pub fn ops(&self) -> Vec<PressOp> {
        self.ops.lock().unwrap().clone()
    }
}

#[async_trait]
impl Press for FakePress {
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
    async fn respond(&self, out: Rendered, ephemeral: bool) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Respond {
            ephemeral,
            text: text_of(&out),
        });
        Ok(())
    }
    async fn update(&self, out: Rendered) -> Result<(), TransportError> {
        self.ops.lock().unwrap().push(PressOp::Update {
            text: text_of(&out),
            rows: out.rows.len(),
        });
        Ok(())
    }
    async fn open_modal(
        &self,
        modal: serenity::all::CreateModal<'static>,
    ) -> Result<(), TransportError> {
        // The builder's fields are private; its payload is a third-party shape.
        let payload = serde_json::to_value(&modal).expect("a modal serializes");
        let custom_id = payload["custom_id"].as_str().unwrap_or_default().to_owned();
        self.ops.lock().unwrap().push(PressOp::Modal(custom_id));
        self.modals.lock().unwrap().push(modal);
        match self.modal_error.lock().unwrap().clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod press_tests {
    use super::*;
    use serenity::all::CreateModal;

    #[tokio::test]
    async fn fake_press_records_an_opened_modal() {
        let press = FakePress::default();
        press
            .open_modal(CreateModal::new("rooms:rename", "Rename"))
            .await
            .unwrap();
        assert_eq!(press.ops(), vec![PressOp::Modal("rooms:rename".into())]);
        assert_eq!(press.modals.lock().unwrap().len(), 1);
    }
}
