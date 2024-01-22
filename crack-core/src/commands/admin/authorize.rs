use crate::errors::CrackedError;
use crate::guild::settings::GuildSettings;
use crate::utils::check_reply;
use crate::Context;
use crate::Error;
use poise::CreateReply;

/// Authorize a user to use the bot.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn authorize(
    ctx: Context<'_>,
    #[description = "The user id to add to authorized list"] user_id: String,
) -> Result<(), Error> {
    let id = user_id.parse::<u64>().expect("Failed to parse user id");
    let guild_id = ctx.guild_id().unwrap();
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
    let pool = ctx.data().database_pool.clone().ok_or({
        tracing::error!("No database pool");
        CrackedError::Other("No database pool")
    })?;
    guild_settings.save(&pool).await?;

    check_reply(
        ctx.send(
            CreateReply::default()
                .content("User authorized.")
                .reply(true),
        )
        .await,
    );

    Ok(())
}
