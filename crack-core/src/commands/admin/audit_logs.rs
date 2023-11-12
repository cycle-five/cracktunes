use crate::errors::CrackedError;
use crate::Context;
use crate::Error;
use std::io::Write;

/// Retreive audit logs.
#[poise::command(prefix_command, owners_only, ephemeral)]
pub async fn audit_logs(ctx: Context<'_>) -> Result<(), Error> {
    match ctx.guild_id() {
        Some(guild) => {
            let guild = guild.to_partial_guild(&ctx).await?;
            let logs = guild.audit_logs(&ctx, None, None, None, None).await?;
            // open a file to write to
            let mut file = std::fs::File::create(format!("audit_logs_{}.txt", guild.id))?;
            // write the logs to the file
            file.write_all(format!("{:?}", logs).as_bytes())?;
        }
        None => {
            return Result::Err(
                CrackedError::Other("This command can only be used in a guild.").into(),
            );
        }
    }
    Ok(())
}
