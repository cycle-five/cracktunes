use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::ChannelId;
use serenity::all::EditChannel;

/// Set the size of a voice channel.
#[poise::command(
    prefix_command,
    slash_command,
    default_member_permissions = "ADMINISTRATOR",
    guild_only
)]
pub async fn set_vc_size(
    ctx: Context<'_>,
    #[description = "VoiceChannel to edit"] channel: ChannelId,
    #[description = "New max size"] size: u32,
) -> Result<(), Error> {
    let _guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let _res = channel
        .edit(&ctx, EditChannel::new().user_limit(size))
        .await?;
    send_response_poise(
        ctx,
        CrackedMessage::Other(format!("Channel size sent to {size}")),
        true,
    )
    .await
    .map(|_| ())
}
