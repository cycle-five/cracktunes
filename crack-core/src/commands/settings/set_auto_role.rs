use crate::{Context, Error};

/// Set the auto role for the server.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn set_auto_role(
    ctx: Context<'_>,
    #[description = "The role to assign to new users"] auto_role_id_str: String,
) -> Result<(), Error> {
    let auto_role_id = match auto_role_id_str.parse::<u64>() {
        Ok(x) => x,
        Err(e) => {
            ctx.say(format!("Failed to parse role id: {}", e)).await?;
            return Ok(());
        }
    };

    let _res = ctx
        .data()
        .guild_settings_map
        .write()
        .unwrap()
        .entry(ctx.guild_id().unwrap())
        .and_modify(|e| {
            e.set_auto_role(Some(auto_role_id));
        });

    ctx.say(format!("Auto role set to {}", auto_role_id))
        .await?;
    Ok(())
}
