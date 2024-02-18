use crate::errors::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use chrono::DateTime;
use chrono::Utc;
use serenity::all::Mentionable;
use serenity::all::UserId;
use serenity::builder::EditMember;
use std::time::Duration;

/// Kick command to kick a user from the server based on their ID
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, ephemeral, owners_only)]
pub async fn kick(ctx: Context<'_>, user_id: UserId) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let reply_with_embed = ctx
        .data()
        .get_guild_settings(guild_id)
        .map(|x| x.reply_with_embed)
        .ok_or(CrackedError::Other("No guild settings"))?;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    if let Err(e) = guild.kick(&ctx, user_id).await {
        send_response_poise(
            ctx,
            CrackedMessage::Other(format!("Failed to kick user: {}", e)),
            reply_with_embed,
        )
        .await?;
    } else {
        // Send success message
        send_response_poise(
            ctx,
            CrackedMessage::UserKicked { user_id },
            reply_with_embed,
        )
        .await?;
    }
    Ok(())
}

use std::fs::read_to_string;

fn read_lines(filename: &str) -> Vec<String> {
    let mut result = Vec::new();

    for line in read_to_string(filename).unwrap().lines() {
        result.push(line.to_string())
    }

    result
}

/// Kick command to kick all users from the server
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, ephemeral, owners_only)]
pub async fn rename_all(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    // let reply_with_embed = ctx
    //     .data()
    //     .get_guild_settings(guild_id)
    //     .map(|x| x.reply_with_embed)
    //     .ok_or(CrackedError::NoGuildSettings)?;
    // load names from file
    let mut names: Vec<String> = read_lines("names.txt")
        .iter()
        .map(|s| s.to_string().trim().to_string())
        .collect::<Vec<String>>();
    // let n = names.len();
    ctx.say("Welcome to Bell Labs...").await?;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    let members = guild.members(&ctx, None, None).await?;
    let mut backoff = Duration::from_secs(1);
    // Half a second
    let sleep = Duration::from_millis(100);
    let to_skip = [];

    for member in members {
        if to_skip.contains(&member.user.id) {
            continue;
        }
        let r = rand::random::<usize>() % names.len();
        let _until =
            DateTime::from_timestamp((Utc::now() + Duration::from_secs(60)).timestamp(), 0)
                .unwrap();
        if let Err(e) = guild
            .edit_member(
                &ctx,
                member.user.id,
                EditMember::new().nickname(names.remove(r).clone()),
            )
            .await
        {
            // Sleep for a bit to avoid rate limiting
            tokio::time::sleep(backoff).await;
            backoff *= 2;
            // Handle error, send error message
            ctx.say(format!(
                "No cocaine in the lab! {}: {}",
                member.mention(),
                e
            ))
            .await?;
        } else {
            if backoff > Duration::from_secs(1) {
                backoff /= 2;
            }
            tokio::time::sleep(sleep).await;
            // Send success message
            ctx.say(format!(
                "Welcome to computer revolution {}!",
                member.mention()
            ))
            .await?;
        }
    }
    Ok(())
}

// pub async fn kick_by_ids(
//     ctx: Context<'_>,
//     guild_id: GuildId,
//     user_id: UserId,
// ) -> Result<(), Error> {
//     let user_id = UserId::new("207533376314277888");
//     let user_id2 = UserId::new("733028372992753895");
//     let guild = guild_id.to_partial_guild(&ctx).await?;
//     if let Err(e) = guild.kick(&ctx, user_id).await {
//         // Handle error, send error message
//         send_response_poise(
//             ctx,
//             CrackedMessage::Other(format!("Failed to kick user: {}", e)),
//         )
//         .await?;
//     } else {
//         // Send success message
//         send_response_poise(ctx, CrackedMessage::UserKicked { user_id }).await?;
//     }
//     Ok(())
// }
