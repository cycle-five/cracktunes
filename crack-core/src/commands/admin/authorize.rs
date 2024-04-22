use crate::errors::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::UserId;

/// Utilizes the permissions v2 `required_permissions` field
#[poise::command(slash_command, required_permissions = "ADMINISTRATOR")]
pub async fn check_admin(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("Authorized.").await?;

    Ok(())
}

/// Authorize a user to use the bot.
#[poise::command(prefix_command, slash_command, required_permissions = "ADMINISTRATOR")]
#[cfg(not(tarpaulin_include))]
pub async fn authorize(
    ctx: Context<'_>,
    #[description = "The user id to add to authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // let data = ctx.data();

    let guild_settings = ctx
        .data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| {
            e.authorized_users.insert(id, 0);
        })
        .or_insert({
            let settings = GuildSettings::new(
                ctx.guild_id().unwrap(),
                Some(&ctx.data().bot_settings.get_prefix()),
                None,
            )
            .authorize_user(id.try_into().unwrap())
            .clone();
            settings
        })
        .clone();
    let pool = ctx
        .data()
        .database_pool
        .clone()
        .ok_or(CrackedError::Other("No database pool"))?;
    guild_settings.save(&pool).await?;

    let user_id = UserId::new(id);
    let user_name = ctx
        .http()
        .get_user(user_id)
        .await
        .map(|u| u.name)
        .unwrap_or_else(|_| "Unknown".to_string());
    let guild_name = guild_id
        .to_partial_guild(ctx.http())
        .await
        .map(|g| g.name)
        .unwrap_or_else(|_| "Unknown".to_string());
    send_response_poise(
        ctx,
        CrackedMessage::UserAuthorized {
            user_id,
            user_name,
            guild_id,
            guild_name,
        },
        true,
    )
    .await
    .map(|m| ctx.data().add_msg_to_cache(guild_id, m))
    .map(|_| ())
    .map_err(Into::into)
}
