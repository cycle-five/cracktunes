use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use poise::serenity_prelude as serenity;
use poise::ReplyHandle;
use serenity::{Http, Message};

/// Trait for a reply handle so that we can store it in an enum.
pub trait ReplyHandleTrait: Send + Sync {
    /// Converts the reply handle into a message.
    fn into_message(
        self: Arc<Self>,
    ) -> Pin<Box<dyn Future<Output = Option<Message>> + Send + 'static>>;

    /// Deletes the message associated with the reply handle.
    fn delete(
        self: Arc<Self>,
        ctx: Http,
    ) -> Pin<Box<dyn Future<Output = serenity::Result<()>> + Send + 'static>>;
}

/// Wrapper around poise::ReplyHandle<'a> that implements ReplyHandleTrait.
pub struct ReplyHandleWrapper {
    pub handle: Arc<ReplyHandle<'static>>,
}

impl ReplyHandleTrait for ReplyHandleWrapper {
    // fn into_message(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Pin<Box<Option<Message>>>> + Send>> {
    fn into_message(
        self: Arc<Self>,
    ) -> Pin<Box<dyn Future<Output = Option<Message>> + Send + 'static>> {
        // let handle: Arc<ReplyHandle<'static>> = Arc::clone(&self.handle);
        let handle = Arc::clone(&self.handle);
        Box::pin(async move {
            <poise::ReplyHandle<'_> as Clone>::clone(&handle)
                .into_message()
                .await
                .ok()
        })
    }

    fn delete(
        self: Arc<Self>,
        ctx: Http,
    ) -> Pin<Box<dyn Future<Output = serenity::Result<()>> + Send + 'static>> {
        // TODO: This does not work with ephemeral messages. We need to take a full
        // poise::Context instead of just the Http client here, which is going to
        // be more complex.
        // let handle: Arc<ReplyHandle<'static>> = Arc::clone(&self.handle);
        let handle = Arc::clone(&self.handle);
        Box::pin(async move {
            match <poise::ReplyHandle<'_> as Clone>::clone(&handle)
                .into_message()
                .await
            {
                Ok(x) => x.delete(ctx).await,
                Err(_) => Ok(()),
            }
        })
    }
}

/// Empty "wrapper" that implements ReplyHandleTrait for testing purposes.
struct ReplyHandleWrapperSimple;

impl ReplyHandleTrait for ReplyHandleWrapperSimple {
    fn into_message(
        self: Arc<Self>,
    ) -> Pin<Box<dyn Future<Output = Option<Message>> + Send + 'static>> {
        Box::pin(async move { None })
    }

    fn delete(
        self: Arc<Self>,
        _ctx: Http,
    ) -> Pin<Box<dyn Future<Output = serenity::Result<()>> + Send + 'static>> {
        Box::pin(async move { Ok(()) })
    }
}

/// Enum that can hold either a message or a reply handle.
#[derive(Clone)]
pub enum MessageOrReplyHandle {
    Message(Message),
    ReplyHandle(Arc<dyn ReplyHandleTrait>),
}

impl std::fmt::Debug for MessageOrReplyHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageOrReplyHandle::Message(message) => write!(f, "Message: {:?}", message),
            MessageOrReplyHandle::ReplyHandle(_) => write!(f, "ReplyHandle"),
        }
    }
}

impl From<Message> for MessageOrReplyHandle {
    fn from(message: Message) -> Self {
        MessageOrReplyHandle::Message(message)
    }
}

impl From<Arc<dyn ReplyHandleTrait>> for MessageOrReplyHandle {
    fn from(handle: Arc<dyn ReplyHandleTrait>) -> Self {
        MessageOrReplyHandle::ReplyHandle(handle)
    }
}

impl From<ReplyHandleWrapper> for MessageOrReplyHandle {
    fn from(handle: ReplyHandleWrapper) -> Self {
        MessageOrReplyHandle::ReplyHandle(Arc::new(handle))
    }
}

/// Struct that holds a message or a reply handle.
#[derive(Debug)]
pub struct Container {
    handle: MessageOrReplyHandle,
}

/// Implementation of Container.
impl Container {
    fn new(handle: MessageOrReplyHandle) -> Self {
        Container { handle }
    }

    fn get_handle(&self) -> &MessageOrReplyHandle {
        &self.handle
    }
}

pub async fn run() {
    let message = Message::default();
    let handle = MessageOrReplyHandle::Message(message);
    let container = Container::new(handle);

    let _ = container.get_handle();

    println!("{:?}", container);
    // To use ReplyHandle:
    // let reply_handle = poise::ReplyHandle::new(); // assuming a way to create one
    let wrapped_handle = ReplyHandleWrapperSimple;
    let handle = MessageOrReplyHandle::ReplyHandle(Arc::new(wrapped_handle));
    let container = Container::new(handle);

    println!("{:?}", container);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_into_message() {
        let message = Message::default();
        let _handle = MessageOrReplyHandle::Message(message);

        // let message = handle.into_message().await.unwrap();
        // assert_eq!(message.id, 0);
    }

    #[tokio::test]
    async fn test_delete() {
        // let message = Message::default();
        let wrapper = Arc::new(ReplyHandleWrapperSimple);
        let x = wrapper.delete(Http::new(&"".to_string())).await;

        assert!(x.is_ok());
    }

    #[tokio::test]
    async fn test_container() {
        // Example usage
        let message = Message::default();
        let handle = MessageOrReplyHandle::Message(message);

        let container = Container::new(handle);

        let _ = container.get_handle();

        println!("{:?}", container);
        // To use ReplyHandle:
        // let reply_handle = poise::ReplyHandle::new(); // assuming a way to create one
        let wrapped_handle = ReplyHandleWrapperSimple;
        let handle = MessageOrReplyHandle::ReplyHandle(Arc::new(wrapped_handle));
        let container = Container::new(handle);

        println!("{:?}", container);
    }
}
