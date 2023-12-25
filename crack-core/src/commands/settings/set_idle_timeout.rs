use crate::utils::check_reply;
use crate::Context;
use crate::Error;
use poise::CreateReply;

/// Set the idle timeout for the bot in vc.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn set_idle_timeout(
    ctx: Context<'_>,
    #[description = "Idle timeout for the bot in minutes."] timeout: u32,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    let timeout = timeout * 60;

    data.guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.timeout = timeout);

    check_reply(
        ctx.send(
            CreateReply::default()
                .content(format!("timeout set to {} seconds", timeout))
                .reply(true),
        )
        .await,
    );

    Ok(())
}
