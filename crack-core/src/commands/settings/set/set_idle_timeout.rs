use crate::guild::settings::GuildSettings;
use crate::utils::check_reply;
use crate::utils::get_guild_name;
use crate::Context;
use crate::Error;
use poise::CreateReply;

/// Set the idle timeout for the bot in vc.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral, aliases("set_idle_timeout"))]
pub async fn idle_timeout(
    ctx: Context<'_>,
    #[description = "Idle timeout for the bot in minutes."] timeout: u32,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap();
    let data = ctx.data();

    let timeout = timeout * 60;

    let _res = data
        .guild_settings_map
        .write()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.timeout = timeout)
        .or_insert_with(|| {
            GuildSettings::new(
                guild_id,
                Some(&ctx.data().bot_settings.get_prefix()),
                get_guild_name(ctx.serenity_context(), guild_id),
            )
            .with_timeout(timeout)
            .clone()
        })
        .welcome_settings
        .clone();
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
