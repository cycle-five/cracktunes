use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::UserId;

/// Deauthorize a user from using the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    required_permissions = "ADMINISTRATOR",
    owners_only
)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[description = "The user id to remove from the authorized list"] user_id: UserId,
) -> Result<(), Error> {
    let id = user_id.get();
    let guild_id = ctx.guild_id().unwrap();

    // TODO: Test to see how expensive this is.
    // TODO: Make this into a function, it's used other places.
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

    let res = ctx
        .data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|settings| {
            settings.authorized_users.remove(&id);
        })
        .or_insert({
            crate::guild::settings::GuildSettings::new(
                ctx.guild_id().unwrap(),
                Some(&ctx.data().bot_settings.get_prefix()),
                Some(guild_name.clone()),
            )
            .clone()
        })
        .clone();
    tracing::info!("User Deauthorized: UserId = {}, GuildId = {}", id, res);

    let msg = send_response_poise(
        ctx,
        CrackedMessage::UserDeauthorized {
            user_id,
            user_name,
            guild_id,
            guild_name,
        },
        true,
    )
    .await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}
