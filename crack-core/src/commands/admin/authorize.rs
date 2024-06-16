use crate::errors::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_reply;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::Mentionable;
use serenity::all::User;

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
    #[description = "The user to add to authorized list"] user: User,
) -> Result<(), Error> {
    // let id = user_id.parse::<u64>().expect("Failed to parse user id");

    use crate::messaging::messages::UNKNOWN;
    let mention = user.mention();
    let id = user.id;
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;

    let guild_settings = ctx
        .data()
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.authorized_users.insert(id.get(), 0);
        })
        .or_insert({
            let settings =
                GuildSettings::new(guild_id, Some(&ctx.data().bot_settings.get_prefix()), None)
                    .authorize_user(id.into())
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

    let guild_name = guild_id
        .to_partial_guild(ctx.http())
        .await
        .map(|g| g.name)
        .unwrap_or_else(|_| UNKNOWN.to_string());
    let _ = send_reply(
        &ctx,
        CrackedMessage::UserAuthorized {
            id,
            mention,
            guild_id,
            guild_name,
        },
        true,
    )
    .await?;
    Ok(())
}
