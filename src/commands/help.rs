use crate::{Context, Error};

/// Show this help menu.
#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<(), Error> {
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            extra_text_at_bottom: "This is a friendly crack smoking parrot that plays music.",
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

/// Get information about the servers this bot is in.
#[poise::command(slash_command, prefix_command, owners_only)]
pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::servers(ctx).await?;
    Ok(())
}
