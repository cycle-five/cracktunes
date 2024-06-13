use crate::commands::cmd_check_music;
use crate::guild::operations::GuildSettingsOperations;
use crate::{messaging::message::CrackedMessage, utils::send_response_poise, Context, Error};

fn autoplay_help() -> String {
    "Toggle music autoplay".to_string()
}

/// Toggle music autoplay.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    aliases("ap"),
    category = "Music",
    check = "cmd_check_music",
//    help = "Toggle music autoplay"
    help_text_fn = "autoplay_help",
)]
pub async fn autoplay(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();

    let autoplay = ctx.data().get_autoplay(guild_id).await;
    ctx.data().set_autoplay(guild_id, !autoplay).await;

    let msg = if autoplay {
        send_response_poise(ctx, CrackedMessage::AutoplayOff, true)
    } else {
        send_response_poise(ctx, CrackedMessage::AutoplayOn, true)
    }
    .await?;
    ctx.data().add_msg_to_cache(guild_id, msg).await;
    Ok(())
}
