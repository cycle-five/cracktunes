use std::sync::Arc;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::UNKNOWN_LIT;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use serenity::all::Channel;
use serenity::all::Context as SerenityContext;
use serenity::all::EditChannel;

/// Set the size of a voice channel.
#[poise::command(
    category = "Admin",
    rename = "setvcsize",
    prefix_command,
    slash_command,
    required_permissions = "ADMINISTRATOR",
    guild_only
)]
pub async fn set_vc_size(
    ctx: Context<'_>,
    #[description = "VoiceChannel to edit"] channel: Channel,
    #[description = "New max size"] size: u32,
    #[flag]
    #[description = "Show the help menu."]
    help: bool,
) -> Result<(), Error> {
    if help {
        return crate::commands::help::wrapper(ctx).await;
    }
    // let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let _ = channel
        .id()
        .edit(&ctx, EditChannel::new().user_limit(size))
        .await?;
    send_reply(
        &ctx,
        CrackedMessage::Other(format!("Channel size sent to {size}")),
        true,
    )
    .await
    .map(|_| ())
    .map_err(Into::into)
}

/// Set the size of a voice channel.
pub async fn set_vc_size_internal(
    ctx: Arc<SerenityContext>,
    channel: Channel,
    size: u32,
) -> Result<CrackedMessage, CrackedError> {
    let id = channel.id();
    let name = id
        .name(&ctx)
        .await
        .unwrap_or_else(|_| UNKNOWN_LIT.to_string());
    if let Err(e) = id.edit(&ctx, EditChannel::new().user_limit(size)).await {
        Err(CrackedError::FailedToSetChannelSize(
            name,
            id,
            size,
            e.into(),
        ))
    } else {
        Ok(CrackedMessage::ChannelSizeSet { name, id, size })
    }
}
