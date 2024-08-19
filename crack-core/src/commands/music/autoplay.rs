use crate::commands::cmd_check_music;
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
    aliases("ap")
)]
pub async fn autoplay(
    ctx: Context<'_>,
    #[description = "Optional value to set autoplay to."] value: Option<bool>,
) -> Result<(), Error> {
    toggle_autoplay(ctx, value).await
}

/// Toggle music autoplay.
pub async fn toggle_autoplay(ctx: Context<'_>, value: Option<bool>) -> Result<(), Error> {
    fn autoplay_msg(autoplay: bool) -> CrackedMessage {
        if autoplay {
            CrackedMessage::AutoplayOn
        } else {
            CrackedMessage::AutoplayOff
        }
    }

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let autoplay = if let Some(value_unwrap) = value {
        value_unwrap
    } else {
        !ctx.data().get_autoplay(guild_id).await
    };
    let _ = ctx.data().set_autoplay(guild_id, autoplay).await;
    let _ = ctx.data().set_autoplay_setting(guild_id, autoplay).await;

    send_reply(&ctx, autoplay_msg(autoplay), true).await?;
    Ok(())
}
