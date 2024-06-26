use crate::{
    errors::CrackedError, guild::settings::GuildSettings, utils::get_guild_name, Context, Error,
};

/// Set the auto role for the server.
#[poise::command(prefix_command, ephemeral, required_permissions = "ADMINISTRATOR")]
pub async fn auto_role(
    ctx: Context<'_>,
    #[description = "The role to assign to new users"] auto_role_id_str: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let auto_role_id = match auto_role_id_str.parse::<u64>() {
        Ok(x) => x,
        Err(e) => {
            ctx.say(format!("Failed to parse role id: {}", e)).await?;
            return Ok(());
        },
    };

    let res = ctx
        .data()
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.set_auto_role(Some(auto_role_id));
        })
        .or_insert_with(|| {
            GuildSettings::new(
                guild_id,
                Some(&ctx.data().bot_settings.get_prefix()),
                get_guild_name(ctx.serenity_context(), guild_id),
            )
            .with_auto_role(Some(auto_role_id))
        })
        .welcome_settings
        .clone();
    res.unwrap()
        .save(&ctx.data().database_pool.clone().unwrap(), guild_id.get())
        .await?;

    ctx.say(format!("Auto role set to {}", auto_role_id))
        .await?;
    Ok(())
}
