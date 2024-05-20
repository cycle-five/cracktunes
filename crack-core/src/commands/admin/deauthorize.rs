use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use poise::serenity_prelude::Mentionable;

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
    #[description = "The user id to remove from the authorized list"] user: serenity::all::User,
) -> Result<(), Error> {
    let user_id = user.id;
    let id = user_id;
    let guild_id = ctx.guild_id().unwrap();

    // TODO: Test to see how expensive this is.
    // TODO: Make this into a function, it's used other places.
    let guild_name = guild_id
        .to_partial_guild(ctx)
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
            settings.authorized_users.remove(&id.get());
        })
        .or_insert({
            crate::guild::settings::GuildSettings::new(
                guild_id,
                Some(&ctx.data().bot_settings.get_prefix()),
                Some(guild_name.clone()),
            )
            .clone()
        })
        .clone();
    tracing::info!("User Deauthorized: UserId = {}, GuildId = {}", id, res);

    let mention = user.mention();
    let msg = send_response_poise(
        ctx,
        CrackedMessage::UserDeauthorized {
            id,
            mention,
            guild_id,
            guild_name,
        },
        true,
    )
    .await?;

    ctx.data().add_msg_to_cache(guild_id, msg);

    Ok(())
}
