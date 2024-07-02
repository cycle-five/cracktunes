use crate::errors::CrackedError;
use crate::Context;
use crate::Error;
use serenity::all::ChannelType;
use serenity::all::{ChannelId, UserId};
use serenity::builder::EditMember;

/// Move usrers to a given channel.
#[poise::command(
    rename = "move_users_to",
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn move_users_to(
    ctx: Context<'_>,
    #[description = "Users to move"] user_ids: Vec<UserId>,
    #[description = "Channel to move users to"] chan_id: ChannelId,
) -> Result<(), Error> {
    // Check if the Channel's are voice channels
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let channels = guild_id.channels(ctx).await?;
    let guild_chan_to = channels
        .get(&chan_id)
        .ok_or(CrackedError::Other("Channel not found"))?;
    if guild_chan_to.kind != ChannelType::Voice {
        return Result::Err(
            CrackedError::Other("This command can only be used in a guild.").into(),
        );
    }

    for user_id in user_ids.iter() {
        let mut member = ctx.http().get_member(guild_id, *user_id).await?;

        let _ = member
            .edit(&ctx, EditMember::new().voice_channel(guild_chan_to.id))
            .await;
    }

    Ok(())
}
