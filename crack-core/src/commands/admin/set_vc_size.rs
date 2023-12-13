use std::sync::Arc;

use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::EditChannel;
use serenity::all::{ChannelId, Context as SerenityContext, GuildId};

/// Set the size of a voice channel.
#[poise::command(prefix_command, guild_only, owners_only)]
pub async fn set_vc_size(
    ctx: Context<'_>,
    #[description = "VoiceChannel to eidt"] channel: ChannelId,
    #[description = "New max size"] size: u32,
) -> Result<(), Error> {
    let _guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let _res = channel
        .edit(&ctx, EditChannel::new().user_limit(size))
        .await?;
    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Channel size sent to {size}")),
    )
    .await
    .map(|_| ())
}
