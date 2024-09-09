use poise::serenity_prelude::Message;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use poise::serenity_prelude::Http;

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

/// Trait for a reply handle.
pub trait ReplyHandleTrait {
    /// Converts the reply handle into a message.
    fn into_message(&self) -> Pin<Box<dyn Future<Output = Option<Message>>>>;
    /// Deletes the message associated with the reply handle.
    fn delete(&self, ctx: Http) -> Pin<Box<dyn Future<Output = Result<(), serenity::Error>>>;
}

// Example implementation of the trait for poise::ReplyHandle<'a>
pub struct ReplyHandleWrapper<'a>(pub poise::ReplyHandle<'a>);


impl ReplyHandleTrait for ReplyHandleWrapper<'_> {
    fn into_message(&self) -> Pin<Box<dyn Future<Output = Option<Message>> + Send + '_>> {
        Box::pin(async move { self.0.clone().into_message().await.ok() })
    }

    fn delete(&self, ctx: Http) -> Pin<Box<dyn Future<Output = Result<(), serenity::Error>> + Send + '_>> {
        Box::pin(async move { 
            match self.0.clone().into_message().await {
                Some(x) => x.delete(ctx).await,
                None => Ok(())
            }
        })
    }
}

// impl<'a> ReplyHandleTrait for ReplyHandleWrapper<'a> {
//     fn into_message(&self) -> Pin<Box<dyn Future<Output = Option<Message>> + '_>> {
//         Box::pin(async move { self.0.clone().into_message().await.ok() })
//     }

//     fn delete(&self, ctx: Http) -> Pin<Box<dyn Future<Output = Result<(), serenity::Error>>>> {
//         Box::pin(async move { self.0.clone().into_message().await.map(|x| x.delete(ctx).await )})
//     }
// }

pub struct Container {
    pub handle: MessageOrReplyHandle,
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Container: {:?}", self.handle)
    }
}

impl Container {
    pub fn new(handle: MessageOrReplyHandle) -> Self {
        Container { handle }
    }
}

impl MessageOrReplyHandle {
    pub async fn into_message(&self) -> Option<Message> {
        match self {
            MessageOrReplyHandle::Message(msg) => Some(msg.clone()),
            MessageOrReplyHandle::ReplyHandle(handle) => handle.into_message().await,
        }
    }

    pub async fn delete(&self, ctx: Http) {
        match self {
            MessageOrReplyHandle::Message(msg) => {
                let _ = msg.delete(&ctx).await;
            },
            MessageOrReplyHandle::ReplyHandle(handle) => {
                let handle: ReplyHandleWrapper = handle.into()
                let _ = handle.delete(ctx).await;
            },
        }
    }
}

impl From<Message> for MessageOrReplyHandle {
    fn from(msg: Message) -> Self {
        MessageOrReplyHandle::Message(msg)
    }
}

impl<'static> From<poise::ReplyHandle<'static>> for MessageOrReplyHandle {
    fn from(handle: poise::ReplyHandle<'static>) -> Self {
        MessageOrReplyHandle::ReplyHandle(Arc::new(ReplyHandleWrapper::<'static>(handle)))
    }
}

pub async fn run() {
    // Example usage
    let message = Message::default();
    let handle = MessageOrReplyHandle::Message(message);

    let container = Container::new(handle);

    println!("{:?}", container);
    // To use ReplyHandle:
    // let reply_handle = poise::ReplyHandle::new(); // assuming a way to create one
    // let wrapped_handle = ReplyHandleWrapper(reply_handle);
    // let handle = MessageOrReplyHandle::ReplyHandle(Rc::new(wrapped_handle));
    // let container = Container::new(handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_into_message() {
        let message = Message::default();
        let handle = MessageOrReplyHandle::Message(message);

        let message = handle.into_message().await.unwrap();
        assert_eq!(message.id, 0);
    }
}
