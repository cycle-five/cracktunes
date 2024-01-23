use crate::{Context, Error};

// /// Show help message
// #[poise::command(prefix_command, track_edits, category = "Utility")]
// async fn help(
//     ctx: Context<'_>,
//     #[description = "Command to get help for"]
//     #[rest]
//     mut command: Option<String>,
// ) -> Result<(), Error> {
//     // This makes it possible to just make `help` a subcommand of any command
//     // `/fruit help` turns into `/help fruit`
//     // `/fruit help apple` turns into `/help fruit apple`
//     if ctx.invoked_command_name() != "help" {
//         command = match command {
//             Some(c) => Some(format!("{} {}", ctx.invoked_command_name(), c)),
//             None => Some(ctx.invoked_command_name().to_string()),
//         };
//     }
//     let extra_text_at_bottom = "\
// Type `?help command` for more info on a command.
// You can edit your `?help` message to the bot and the bot will edit its response.";

//     let config = HelpConfiguration {
//         show_subcommands: true,
//         show_context_menu_commands: true,
//         ephemeral: true,
//         extra_text_at_bottom,

//         ..Default::default()
//     };
//     poise::builtins::help(ctx, command.as_deref(), config).await?;
//     Ok(())
// }

/// Show the help menu.
#[cfg(not(tarpaulin_include))]
#[poise::command(prefix_command, track_edits, slash_command, category = "Utility")]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    //#[autocomplete = "poise::builtins::autocomplete_command"]
    #[rest]
    mut command: Option<String>,
) -> Result<(), Error> {
    // This makes it possible to just make `help` a subcommand of any command
    // `/fruit help` turns into `/help fruit`
    // `/fruit help apple` turns into `/help fruit apple`
    if ctx.invoked_command_name() != "help" {
        command = match command {
            Some(c) => Some(format!("{} {}", ctx.invoked_command_name(), c)),
            None => Some(ctx.invoked_command_name().to_string()),
        };
    }
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            show_subcommands: false,
            show_context_menu_commands: false,
            extra_text_at_bottom: "This is a friendly crack smoking parrot that plays music.",
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

/// Get information about the servers this bot is in.
#[cfg(not(tarpaulin_include))]
#[poise::command(slash_command, prefix_command, owners_only)]
pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::servers(ctx).await?;
    Ok(())
}
