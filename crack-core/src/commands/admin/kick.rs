//#![feature(const_random)] // This is a nightly feature
use crate::errors::CrackedError;
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use serenity::all::{Mentionable, User};
use serenity::builder::EditMember;
use std::fs::read_to_string;
use std::time::Duration;

/// Kick command to kick a user from the server based on their ID
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    ephemeral,
    required_permissions = "ADMINISTRATOR"
)]
pub async fn kick(
    ctx: Context<'_>,
    #[description = "User to kick."] user: User,
) -> Result<(), Error> {
    let mention = user.mention();
    let id = user.id;
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let as_embed = ctx.data().get_reply_with_embed(guild_id).await;
    let guild = guild_id.to_partial_guild(&ctx).await?;
    if let Err(e) = guild.kick(&ctx, id).await {
        send_reply(
            &ctx,
            CrackedMessage::Other(format!("Failed to kick user: {}", e)),
            as_embed,
        )
        .await?;
    } else {
        // Send success message
        send_reply(&ctx, CrackedMessage::UserKicked { id, mention }, as_embed).await?;
    }
    Ok(())
}

/// Read lines from a file
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
pub async fn changenicks(
    ctx: Context<'_>,
    #[flag]
    #[description = "Don't actually change the names or print anything, just log."]
    dry: bool,
    #[flag]
    #[description = "Don't call out the changes, just log."]
    quiet: bool,
    #[flag]
    #[description = "See the help menu entry."]
    help: bool,
) -> Result<(), Error> {
    use crate::commands::help;
    if help {
        return help::wrapper(ctx).await;
    }

    rename_all_internal(ctx, dry, quiet).await
}

macro_rules! const_random_string {
    ($category:expr) => {{
        use const_random::const_random;
        const fn get_random_item<const N: usize>(arr: [&str; N]) -> &str {
            let index = const_random!(u8) as usize % N;
            arr[index]
        }

        const RESULT: &str = get_random_item($category);
        RESULT
    }};
}

const CALL_TO_ACTION: [&str; 5] = [
    "Shine on!",
    "Ruby on!",
    "Charge your JO Crystals!",
    "Let's get sapphired up!",
    "Don't Ruby my rights!",
];

const EXHASPERATION: [&str; 4] = [
    "Oh no, my diamonds!",
    "If you leave me I'll Diamond",
    "Kick rocks!",
    "I'll smoke your Quartz!",
];

const CELEBRATION: [&str; 2] = ["Diamonds are a girl's best friend!", "Gemtastic!"];

macro_rules! call_to_action {
    () => {
        const_random_string!(CELEBRATION)
    };
}

macro_rules! celebration {
    () => {
        const_random_string!(EXHASPERATION)
    };
}

macro_rules! exhasperation {
    () => {
        const_random_string!(CALL_TO_ACTION)
    };
}

//const BACKOFF_START_SECS: u64 = 1;
const BACKOFF_MAX_SECS: u64 = 2;
const SLEEP_DURATION_MS: u64 = 100;
const BACKOFF_MULTIPLY_FACTOR: u32 = 2;
//const DEFAULT_EMOJI: &str = "💎";
const CUR_EMOJI: &str = "💎";
const CUR_EMOJI_CHAR: char = '💎';
const STATIC_EMOJI: char = '🧪';
const DISCORD_MAX_NAME_LENGTH: usize = 32;

/// Internal function for rename_all.
#[cfg(not(tarpaulin_include))]
pub async fn rename_all_internal(ctx: Context<'_>, dry: bool, quiet: bool) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::GuildOnly)?;
    let mut names: Vec<String> = read_lines("names.txt")
        .iter()
        .map(|s| s.to_string().trim().to_string())
        .filter(|s| s.len() <= DISCORD_MAX_NAME_LENGTH)
        .collect::<Vec<String>>();
    // let n = names.len();
    //let start_backoff_secs = BACKOFF_START_SECS;
    //let sleep_duration_ms = SLEEP_DURATION_MS;
    //let default_emoji = DEFAULT_EMOJI;
    let cur_emoji = CUR_EMOJI;
    let cur_emoji_char = CUR_EMOJI_CHAR;
    let static_emoji = STATIC_EMOJI;

    if !dry {
        ctx.say(call_to_action!()).await?;
    } else {
        tracing::info!("{}", call_to_action!());
    }
    let guild = guild_id.to_partial_guild(&ctx).await?;
    let members = guild.members(&ctx, None, None).await?;
    let mut backoff = Duration::from_secs(BACKOFF_MAX_SECS);
    let sleep = Duration::from_millis(SLEEP_DURATION_MS);
    let to_skip = [];

    for member in members {
        if to_skip.contains(&member.user.id) {
            continue;
        }
        let r = rand::random::<usize>() % names.len();
        let random_name = names.remove(r).clone();
        let (emoji, new_name) = if let Some(cur_nick) = member.user.nick_in(&ctx, guild_id).await {
            let emoji = cur_nick.chars().next().unwrap_or(cur_emoji_char);
            if !emoji.is_ascii() && !emoji.eq(&static_emoji) {
                (emoji.to_string(), random_name)
            } else {
                (cur_emoji.to_string(), random_name)
            }
        } else {
            (cur_emoji.to_string(), random_name)
        };
        if dry {
            tracing::info!("{} -> {} {}", member.user.name, emoji, new_name);
            continue;
        }

        if let Err(e) = guild
            .edit_member(
                &ctx,
                member.user.id,
                EditMember::new().nickname(new_name.clone()),
            )
            .await
        {
            // Sleep for a bit to avoid rate limiting
            tokio::time::sleep(backoff).await;
            backoff *= BACKOFF_MULTIPLY_FACTOR;
            // Handle error, send error message
            let msg_str = format!("{} {}! {}", exhasperation!(), member.user.mention(), e);
            tracing::info!("{}", msg_str);
            if !quiet {
                ctx.say(msg_str).await?;
            }
        } else {
            if backoff > Duration::from_secs(BACKOFF_MAX_SECS) {
                backoff /= BACKOFF_MAX_SECS as u32;
            }
            tokio::time::sleep(sleep).await;
            // Send success message
            let msg_str = format!("{}, {}!", celebration!(), member.user.mention(),);
            tracing::info!("{}", msg_str);
            if !quiet {
                ctx.say(msg_str).await?;
            }
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
//         send_reply(
//             &ctx,
//             CrackedMessage::Other(format!("Failed to kick user: {}", e)),
//         )
//         .await?;
//     } else {
//         // Send success message
//         send_reply(&ctx, CrackedMessage::UserKicked { user_id }).await?;
//     }
//     Ok(())
// }
