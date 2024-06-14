use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use serenity::all::GuildId;
use serenity::all::Mentionable;
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
    let crack_msg = deafen_internal(&ctx, guild_id, user.clone(), true).await?;
    // Handle error, send error message
    let sent_msg = send_reply(ctx, crack_msg, true).await?;
    ctx.data().add_msg_to_cache(guild_id, sent_msg).await;
    Ok(())
}

/// Uneafen a user.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn undeafen(
    ctx: Context<'_>,
    #[description = "User to undeafen"] user: serenity::model::user::User,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let crack_msg = deafen_internal(&ctx, guild_id, user.clone(), false).await?;
    // Handle error, send error message
    let sent_msg = send_reply(ctx, crack_msg, true).await?;
    ctx.data().add_msg_to_cache(guild_id, sent_msg).await;
    Ok(())
}

/// Deafen or undeafen a user.
pub async fn deafen_internal(
    //ctx: Arc<SerenityContext>,
    cache_http: &impl serenity::prelude::CacheHttp,
    guild_id: GuildId,
    user: serenity::model::user::User,
    deafen: bool,
) -> Result<CrackedMessage, Error> {
    let mention = user.clone().mention();
    let id = user.clone().id;
    let msg = if let Err(e) = guild_id
        .edit_member(
            cache_http,
            user.clone().id,
            EditMember::new().deafen(deafen),
        )
        .await
    {
        let msg = if deafen {
            CrackedMessage::UserDeafenedFail { mention, id }
        } else {
            CrackedMessage::UserUndeafenedFail { mention, id }
        };
        tracing::error!("{msg}\n{e}");
        msg
    } else {
        let msg = if deafen {
            CrackedMessage::UserDeafened { mention, id }
        } else {
            CrackedMessage::UserUndeafened { mention, id }
        };
        tracing::info!("{msg}");
        msg
    };
    Ok(msg)
}
