use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::Context;
use crate::Error;
use poise::all::UserId;

/// Deauthorize a user from using the bot.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    slash_command,
    prefix_command,
    default_member_permissions = "ADMINISTRATOR",
    owners_only
)]
pub async fn deauthorize(
    ctx: Context<'_>,
    #[rest]
    #[description = "The user id to remove from the authorized list"]
    user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
    let res = ctx
        .data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|settings| {
            settings.authorized_users.remove(&id);
        })
        .key();
    tracing::info!("User Deauthorized: UserId = {}, GuildId = {}", id, res);

    // TODO: Test to see how expensive this is.
    // TODO: Make this into a function, it's used other places.
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
        CrackedMessage::UserDeauthorized {
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
}
