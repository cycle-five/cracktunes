use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::builder::EditMember;

/// Deafen a user.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn deafen(
    ctx: Context<'_>,
    #[description = "User to deafen"] user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let crack_msg = deafen_internal(ctx.clone(), user.clone()).await?;
    // Handle error, send error message
    let sent_msg = send_response_poise(ctx, crack_msg, true).await?;
    ctx.data().add_msg_to_cache(guild_id, sent_msg);
    Ok(())
}

/// Deafen a user.
pub async fn deafen_internal(
    ctx: Context<'_>,
    user: serenity::model::user::User,
) -> Result<CrackedMessage, Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let msg = if let Err(e) = guild_id
        .edit_member(&ctx, user.clone().id, EditMember::new().deafen(true))
        .await
    {
        let msg = CrackedMessage::UserDeafenedFail {
            user: user.name.clone(),
            user_id: user.clone().id,
        };
        tracing::error!("{msg}\n{e}");
        msg
    } else {
        let msg = CrackedMessage::UserDeafened {
            user: user.name.clone(),
            user_id: user.clone().id,
        };
        tracing::info!("{msg}");
        msg
    };
    Ok(msg)
}
