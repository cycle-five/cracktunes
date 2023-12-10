use crate::utils::check_reply;
use crate::Context;
use crate::Data;
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

    let settings = ctx
        .data()
        .guild_settings_map
        .write()
        .await
        .entry(guild_id)
        .and_modify(|e| {
            e.authorized_users.insert(id);
        })
        .or_default()
        .clone();
    let data: &mut Data = &mut ctx.data().clone();
    // let mut mut_data = data.deref_mut();
    settings.save(data.get_db_pool()).await?;

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
