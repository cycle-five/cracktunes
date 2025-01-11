use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::message::CrackedMessage;
use crate::poise_ext::PoiseContextExt;
use crate::CrackedError;
use crate::{Context, Error};

/// Get the current `volume` and `old_volume` setting for the guild.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Settings",
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn get_volume(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let (vol, old_vol) = data.get_volume(guild_id).await;

    let volume = CrackedMessage::Volume { vol, old_vol };
    ctx.send_reply(volume, true)
        .await
        .map_err(std::convert::Into::into)
        .map(|_| ())
}
