use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use regex::Regex;
use serenity::all::{User, UserId};
use serenity::builder::EditMember;
use std::time::Duration;

/// Timeout a user from the server.
/// FIXME: THIS IS BROKEN FIX
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    guild_only,
    required_permissions = "ADMINISTRATOR",
    ephemeral
)]
pub async fn timeout(
    ctx: Context<'_>,
    #[description = "User to timout."] user: Option<User>,
    #[description = "UserId to timeout"] user_id: Option<UserId>,
    #[description = "Amount of time"] duration: String,
) -> Result<(), Error> {
    // Debugging print the params
    tracing::error!(
        "User: {:?}, User_id: {:?}, Duration: {}",
        user,
        user_id,
        duration
    );

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    tracing::error!("Guild_id: {}", guild_id);

    let user_id = {
        if let Some(user) = user {
            user.id
        } else if let Some(user_id) = user_id {
            user_id
        } else {
            return Err(CrackedError::Other("No user or user_id provided").into());
        }
    };
    tracing::error!("User_id: {}", user_id);

    let timeout_duration = parse_duration(&duration)?;
    tracing::error!("Timeout duration: {:?}", timeout_duration);

    let now = chrono::Utc::now();
    let timeout_until = now + timeout_duration;
    let timeout_until = timeout_until.to_rfc3339();
    let guild = guild_id.to_partial_guild(&ctx).await?;
    tracing::error!(
        "Guild: {:?}, timeout_until: {}, now: {}",
        guild,
        timeout_until,
        now
    );

    if let Err(e) = guild
        .edit_member(
            &ctx,
            user_id,
            EditMember::default().disable_communication_until(timeout_until.clone()),
        )
        .await
    {
        // Handle error, send error message
        tracing::error!("Failed to timeout user: {}", e);
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Failed to timeout user: {}", e)),
            true,
        )
        .await?;
    } else {
        // Send success message
        let msg = CrackedMessage::UserTimeout {
            user: user_id.to_user(&ctx).await?.name,
            user_id: format!("{}", user_id),
            timeout_until: timeout_until.clone(),
        };
        tracing::info!("User timed out: {}", msg);
        send_response_poise(ctx, msg, true).await?;
    }
    Ok(())
}

/// Parse the input string for a duration
fn parse_duration(input: &str) -> Result<Duration, CrackedError> {
    let mut parts = Vec::new();
    let re = Regex::new(r"((\d+)([smhd]))").unwrap();
    for (_, [_, d, u]) in re.captures_iter(input).map(|c| c.extract()) {
        parts.push((d, u));
    }

    if parts.is_empty() {
        return Err(CrackedError::Other("Invalid format"));
    }
    let mut total = Duration::from_secs(0);

    for (d, u) in parts {
        let d = match d.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                return Err(CrackedError::DurationParseError(
                    d.to_string(),
                    u.to_string(),
                ));
            }
        };
        match u {
            "s" => total += Duration::from_secs(d),
            "m" => total += Duration::from_secs(d * 60),
            "h" => total += Duration::from_secs(d * 60 * 60),
            "d" => total += Duration::from_secs(d * 24 * 60 * 60),
            _ => return Err(CrackedError::Other("Invalid time unit")),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("1m").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(60 * 60));
        assert_eq!(
            parse_duration("1d").unwrap(),
            Duration::from_secs(24 * 60 * 60)
        );
        assert_eq!(
            parse_duration("1d1h1m1s").unwrap(),
            Duration::from_secs(24 * 60 * 60 + 60 * 60 + 60 + 1)
        );
    }
}
