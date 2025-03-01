use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use crack_types::CrackedError;
use poise::serenity_prelude::Mentionable;
use serenity::all::User;

/// Ban a user from the server.
// There really doesn't seem to be a good way to restructure commands like this
// in a way that allows for unit testing.
// 1) Almost every call relies on the &ctx, cache, or http, and these are basically
//   impossible to mock.
// 2) Even trying to segragate the logic in the reponse creation pieces is difficult
//    due to the fact that we're using poise to do prefix and slash commands at the
//    same time. This makes creation of the response embeds relient on the type
//    of command and thus the context.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Admin",
    slash_command,
    prefix_command,
    required_permissions = "BAN_MEMBERS",
    required_bot_permissions = "BAN_MEMBERS",
    ephemeral
)]
pub async fn ban(
    ctx: Context<'_>,
    #[description = "User to ban."] user: User,
    #[description = "Number of day to delete messages of the user."] dmd: Option<u8>,
    #[description = "Reason for the ban."] reason: Option<String>,
) -> Result<(), Error> {
    let mention = user.mention();
    let id = user.id;
    let dmd = dmd.unwrap_or(0);
    let reason = reason.unwrap_or("No reason provided".to_string());
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    if let Err(e) = guild_id
        .ban(ctx.http(), user.id, dmd.into(), Some(&reason))
        .await
    {
        // Handle error, send error message
        send_reply(
            &ctx,
            CrackedMessage::Other(format!("Failed to ban user: {e}")),
            true,
        )
        .await?;
    } else {
        // Send success message
        send_reply(&ctx, CrackedMessage::UserBanned { mention, id }, true).await?;
    }
    Ok(())
}
