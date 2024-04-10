use crate::{
    messaging::messages::{INVITE_LINK_TEXT, INVITE_TEXT, INVITE_URL},
    Context, Error,
};
use poise::serenity_prelude::GuildId;

/// Vote link for cracktunes on top.gg
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command)]
pub async fn invite(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id: Option<GuildId> = ctx.guild_id();

    let reply_handle = ctx
        .reply(format!(
            "{} [{}]({})",
            INVITE_TEXT, INVITE_LINK_TEXT, INVITE_URL
        ))
        .await?;

    let msg = reply_handle.into_message().await?;

    guild_id.map(|id| ctx.data().add_msg_to_cache(id, msg));

    Ok(())
}
