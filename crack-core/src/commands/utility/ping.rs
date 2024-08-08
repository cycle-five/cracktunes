use poise::CreateReply;
use serenity::all::{Color, CreateEmbed};

use crate::messaging::message::CrackedMessage;
use crate::poise_ext::MessageInterfaceCtxExt;
use crate::{Context, Error};

/// Ping the bot
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", slash_command, prefix_command)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    ping_internal(ctx).await
}

/// Ping the bot internal function
#[cfg(not(tarpaulin_include))]
pub async fn ping_internal(ctx: Context<'_>) -> Result<(), Error> {
    let start = std::time::Instant::now();
    let msg = ctx.send_reply_embed(CrackedMessage::Pong).await?;
    let end = std::time::Instant::now();
    let msg_str = format!("Pong! ({}ms)", (end - start).as_millis());
    let edited = CreateReply::default().embed(
        CreateEmbed::default()
            .description(msg_str)
            .color(Color::from(CrackedMessage::Pong)),
    );
    msg.edit(ctx, edited).await.map_err(Error::from)
}
