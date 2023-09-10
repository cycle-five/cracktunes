use crate::{
    errors::CrackedError,
    guild::settings::GuildSettingsMap,
    messaging::message::CrackedMessage,
    utils::{check_reply, create_response_poise},
    Context, Error,
};
// use chrono::NaiveTime;
// use date_time_parser::TimeParser;

/// Admin commands.
#[poise::command(
    prefix_command,
    slash_command,
    subcommands("authorize", "deauthorize", "set_idle_timeout", "set_prefix"),
    ephemeral,
    owners_only,
    hide_in_help
)]
pub async fn admin(_ctx: Context<'_>) -> Result<(), Error> {
    tracing::warn!("Admin command called");

    Ok(())
}

/// Set the prefix for the bot.
#[poise::command(prefix_command, slash_command, owners_only, ephemeral, hide_in_help)]
pub async fn set_prefix(
    ctx: Context<'_>,
    #[description = "The prefix to set for the bot"] prefix: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let mut data = ctx.serenity_context().data.write().await;
    let _entry = &data
        .get_mut::<GuildSettingsMap>()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.prefix = prefix.clone())
        .and_modify(|e| e.prefix_up = prefix.to_uppercase());

    let settings = data.get::<GuildSettingsMap>().unwrap().get(&guild_id);

    let _res = settings.map(|s| s.save()).unwrap();

    create_response_poise(
        ctx,
        CrackedMessage::Other(format!("Prefix set to {}", prefix)),
    )
    .await?;

    Ok(())
}

/// Authorize a user to use the bot.
#[poise::command(prefix_command, slash_command, owners_only, ephemeral, hide_in_help)]
pub async fn authorize(
    ctx: Context<'_>,
    #[description = "The user id to add to authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    data.guild_settings_map
        .lock()
        .unwrap()
        .get_mut(&guild_id)
        .expect("Failed to get guild settings map")
        .authorized_users
        .insert(id);

    //ctx.send("User authorized").await;
    check_reply(
        ctx.send(|m| m.content("User authorized.").reply(true))
            .await,
    );

    Ok(())
}

/// Deauthorize a user from using the bot.
#[poise::command(prefix_command, slash_command, owners_only, ephemeral, hide_in_help)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[description = "The user id to remove from the authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();
    let res = data
        .guild_settings_map
        .lock()
        .unwrap()
        .get_mut(&guild_id)
        .expect("Failed to get guild settings map")
        .authorized_users
        .remove(&id);

    if res {
        check_reply(
            ctx.send(|m| m.content("User authorized.").reply(true))
                .await,
        );
        Ok(())
    } else {
        Err(CrackedError::Other("User did not exist in authorized list").into())
    }
}

/// Set the idle timeout for the bot in vc.
#[poise::command(prefix_command, slash_command, owners_only, ephemeral, hide_in_help)]
pub async fn set_idle_timeout(
    ctx: Context<'_>,
    // #[description = "Idle timeout for the bot in minutes."] timeout: String,
    #[description = "Idle timeout for the bot in minutes."] timeout: u32,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    // let timeout = match TimeParser::parse(&timeout) {
    //     Some(time) => time,
    //     None => return Err(CrackedError::ParseTimeFail.into()),
    // };
    // let timeout = timeout
    //     .signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
    //     .num_seconds() as u32;
    let timeout = timeout * 60;

    data.guild_settings_map
        .lock()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.timeout = timeout);

    check_reply(
        ctx.send(|m| {
            m.content(format!("timeout set to {} seconds", timeout))
                .reply(true)
        })
        .await,
    );

    Ok(())
}

/// Kick command to kick a user from the server based on their ID
#[poise::command(prefix_command, slash_command, hide_in_help, owners_only, ephemeral)]
pub async fn kick(ctx: Context<'_>, user_id: serenity::model::id::UserId) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.kick(&ctx, user_id).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to kick user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(ctx, CrackedMessage::UserKicked { user_id }).await?;
            }
        }
        None => {
            create_response_poise(
                ctx,
                CrackedMessage::Other("This command can only be used in a guild.".to_string()),
            )
            .await?;
        }
    }
    Ok(())
}

/// Kick command to kick a user from the server based on their ID
#[poise::command(prefix_command, slash_command, hide_in_help)]
pub async fn ban(
    ctx: Context<'_>,
    user: serenity::model::user::User,
    dmd: Option<u8>,
    reason: Option<String>,
) -> Result<(), Error> {
    let dmd = dmd.unwrap_or(0);
    let reason = reason.unwrap_or("No reason provided".to_string());
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            if let Err(e) = guild.ban_with_reason(&ctx, user.clone(), dmd, reason).await {
                // Handle error, send error message
                create_response_poise(
                    ctx,
                    CrackedMessage::Other(format!("Failed to ban user: {}", e)),
                )
                .await?;
            } else {
                // Send success message
                create_response_poise(
                    ctx,
                    CrackedMessage::UserBanned {
                        user: user.name.clone(),
                        user_id: user.clone().id,
                    },
                )
                .await?;
            }
        }
        None => {
            create_response_poise(
                ctx,
                CrackedMessage::Other("This command can only be used in a guild.".to_string()),
            )
            .await?;
        }
    }
    Ok(())
}
