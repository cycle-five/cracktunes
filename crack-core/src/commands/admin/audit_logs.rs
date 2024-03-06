use crate::errors::CrackedError;
use crate::Context;
use crate::Error;
//use std::io::Write;

/// Retreive audit logs.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn audit_logs(_ctx: Context<'_>) -> Result<(), Error> {
    Err(CrackedError::NotImplemented.into())
    // let guild = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    // let guild = guild.to_partial_guild(&ctx).await?;
    // let logs = guild.audit_logs(&ctx, None, None, None, None).await?;
    // // open a file to write to
    // //FIXME: Figure out a more consistent way to write this file rather than cwd.
    // let mut file = std::fs::File::create(format!("audit_logs_{}.txt", guild.id))?;
    // // write the logs to the file
    // file.write_all(format!("{:?}", logs).as_bytes())?;
    // Ok(())
}
