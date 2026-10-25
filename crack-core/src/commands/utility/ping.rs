#![expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]

use crack_model::Card;
use poise::CreateReply;

use crate::messaging::discord;
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::PoiseContextExt;
use crate::{Context, Error};

/// Ping the bot
#[poise::command(category = "Utility", slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ping_internal(ctx).await
}

/// Ping the bot internal function
pub async fn ping_internal(ctx: Context<'_>) -> Result<(), Error> {
    let start = std::time::Instant::now();
    let msg = ctx.send_reply_embed(CrackedMessage::Pong).await?;
    let end = std::time::Instant::now();
    let msg_str = format!("Pong! ({}ms)", (end - start).as_millis());
    let edited = CreateReply::default().embed(discord::embed(&pong_card(msg_str)));
    msg.edit(ctx, edited).await.map_err(Error::from)
}

/// The reply `/ping` settles on once it has the timing.
fn pong_card(description: String) -> Card {
    Card {
        description: Some(description),
        colour: Some(CrackedMessage::Pong.colour()),
        ..Card::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pong_card_renders_as_before() {
        let wire = serde_json::to_string(&discord::embed(&pong_card("Pong! (12ms)".into())));
        assert_eq!(
            wire.unwrap(),
            r#"{"type":"rich","description":"Pong! (12ms)","color":3447003}"#
        );
    }
}
