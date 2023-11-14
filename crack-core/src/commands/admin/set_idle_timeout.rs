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

    // let timeout = match TimeParser::parse(&timeout) {
    //     Some(time) => time,
    //     None => return Err(CrackedError::ParseTimeFail.into()),
    // };
    // let timeout = timeout
    //     .signed_duration_since(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
    //     .num_seconds() as u32;
    let timeout = timeout * 60;

    data.guild_settings_map
        .lock()
        .unwrap()
        .entry(guild_id)
        .and_modify(|e| e.timeout = timeout);

    check_reply(
        ctx.send(
            CreateReply::new()
                .content(format!("timeout set to {} seconds", timeout))
                .reply(true),
        )
        .await,
    );

    Ok(())
}
