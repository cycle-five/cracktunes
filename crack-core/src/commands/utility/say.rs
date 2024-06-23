use crate::commands::help;
use crate::{Context, Error};
use serenity::all::{Channel, ChannelId};

/// Have the bot say something in a channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Utility",
    slash_command,
    prefix_command,
    owners_only,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn say_channel(
    ctx: Context<'_>,
    #[flag]
    #[description = "show the help menu for this command."]
    help: bool,
    #[description = "Channel to send the message to"] chan: Channel,
    #[description = "Message to send"] msg: String,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    say_internal(ctx, chan.id(), msg).await
}

/// Have the bot say something in a channel, by id.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Utility",
    slash_command,
    prefix_command,
    owners_only,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn say_channel_id(
    ctx: Context<'_>,
    #[flag]
    #[description = "show the help menu for this command."]
    help: bool,
    #[description = "Channel ID of channel to send message to"] chan: ChannelId,
    #[description = "Message to send"] msg: String,
) -> Result<(), Error> {
    if help {
        return help::wrapper(ctx).await;
    }
    say_internal(ctx, chan, msg).await
}

/// Internal say function.
pub async fn say_internal(ctx: Context<'_>, chan_id: ChannelId, msg: String) -> Result<(), Error> {
    chan_id.say(&ctx.serenity_context(), msg).await?;
    Ok(())
}
