use crate::errors::CrackedError;
use crate::Context;
use crate::Error;
use serenity::all::ChannelType;
use serenity::all::GuildChannel;
use serenity::all::Member;
use serenity::builder::EditMember;

/// Move users between VCs.
#[poise::command(
    rename = "move",
    prefix_command,
    owners_only,
    ephemeral,
    subcommands("move_users")
)]
pub async fn move_(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// Move users between VCs.
#[poise::command(
    rename = "users",
    prefix_command,
    owners_only,
    ephemeral,
    subcommands("move_users_to")
)]
pub async fn move_users(
    _ctx: Context<'_>,
    #[description = "Users to move"] _user: Vec<Member>,
) -> Result<(), Error> {
    Ok(())
}

#[poise::command(rename = "to", prefix_command, owners_only, ephemeral)]
pub async fn move_users_to(
    ctx: Context<'_>,
    #[description = "Users to move"] users: Vec<Member>,
    #[description = "Channel to move users to"] guild_chan_to: GuildChannel,
) -> Result<(), Error> {
    // Check if the Channel's are voice channels
    if guild_chan_to.kind != ChannelType::Voice {
        return Result::Err(
            CrackedError::Other("This command can only be used in a guild.").into(),
        );
    }

    for member in users.clone().iter_mut() {
        let _ = member
            .edit(ctx, EditMember::new().voice_channel(guild_chan_to.id))
            .await;
    }

    Ok(())
}
