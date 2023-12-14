use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use serenity::all::{User, UserId};

/// Timeout a user from the server.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn timeout(
    ctx: Context<'_>,
    #[description = "User to timout."] user: Option<User>,
    #[description = "UserId to timeout"] user_id: Option<UserId>,
    #[description = "Amount of time"] duration: String,
) -> Result<(), Error> {
    use serenity::builder::EditMember;

    let user_id = {
        if let Some(user) = user {
            user.id
        } else if let Some(user_id) = user_id {
            user_id
        } else {
            return Err(CrackedError::Other("No user or user_id provided").into());
        }
    };
    let timeout_duration = {
        let mut timeout_duration = 0;
        let mut duration = duration.split(" ");
        while let Some(d) = duration.next() {
            let d = d
                .parse::<u64>()
                .map_err(|_| CrackedError::Other("Invalid duration."))?;
            match duration.next() {
                Some("d") => timeout_duration += d * 24 * 60 * 60,
                Some("h") => timeout_duration += d * 60 * 60,
                Some("m") => timeout_duration += d * 60,
                Some("s") => timeout_duration += d,
                _ => return Err(CrackedError::Other("Invalid duration.").into()),
            }
        }
        timeout_duration
    };

    let now = chrono::Utc::now();
    let timeout_until = now + chrono::Duration::seconds(timeout_duration as i64);
    let timeout_until = timeout_until.to_rfc3339();
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    if let Err(e) = guild
        .edit_member(
            &ctx,
            user_id,
            EditMember::default().disable_communication_until(timeout_until.clone()),
        )
        .await
    {
        // Handle error, send error message
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Failed to timeout user: {}", e)),
        )
        .await?;
    } else {
        // Send success message
        send_response_poise(
            ctx,
            CrackedMessage::UserTimeout {
                user: user_id.to_user(&ctx).await?.name,
                user_id: format!("{}", user_id),
                timeout_until: timeout_until.clone(),
            },
        )
        .await?;
    }
    Ok(())
}
