use crate::commands::{cmd_check_music, sub_help as help};
use crate::guild::operations::GuildSettingsOperations;
use crate::{messaging::message::CrackedMessage, utils::send_reply, Context, CrackedError, Error};

/// Toggle music autoplay.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    check = "cmd_check_music",
    slash_command,
    prefix_command,
    guild_only,
    aliases("ap"),
    subcommands("help")
)]
pub async fn autoplay(ctx: Context<'_>) -> Result<(), Error> {
    toggle_autoplay(ctx).await
}

/// Toggle music autoplay.
pub async fn toggle_autoplay(ctx: Context<'_>) -> Result<(), Error> {
    fn autoplay_msg(autoplay: bool) -> CrackedMessage {
        if autoplay {
            CrackedMessage::AutoplayOff
        } else {
            CrackedMessage::AutoplayOn
        }
    }

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let autoplay = ctx.data().get_autoplay(guild_id).await;
    ctx.data().set_autoplay(guild_id, !autoplay).await;

    send_reply(ctx, autoplay_msg(autoplay), true).await?;
    Ok(())
}
