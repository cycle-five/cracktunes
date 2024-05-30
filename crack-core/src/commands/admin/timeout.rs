use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::Mentionable;
use regex::Regex;
use serenity::all::User;
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
    #[description = "User to timout."] user: User,
    #[description = "Amount of time"] duration: String,
) -> Result<(), Error> {
    // Debugging print the params
    let id = user.id;
    let mention = user.mention();

    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let timeout_duration = parse_duration(&duration)?;
    tracing::info!("Timeout duration: {:?}", timeout_duration);

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
            id,
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
            id,
            mention,
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
            },
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
