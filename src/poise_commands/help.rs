use crate::{Context, Error};

/// Show this help menu
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

#[cfg(feature = "cache")]
#[poise::command(slash_command, prefix_command)]
pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::servers(ctx).await?;
    Ok(())
}

// #[poise::command(slash_command, prefix_command)]
// pub async fn help(ctx: Context<'_>, command: Option<String>) -> Result<(), Error> {
//     let configuration = poise::builtins::HelpConfiguration {
//         // [configure aspects about the help message here]
//         ..Default::default()
//     };
//     poise::builtins::help(ctx, command.as_deref(), configuration).await?;
//     Ok(())
// }
