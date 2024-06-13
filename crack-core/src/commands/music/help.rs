use crate::commands::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::send_response_poise;
use crate::{require, Context, Data, Error};

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
#[allow(clippy::unused_async)]
pub async fn autocomplete(
    ctx: poise::ApplicationContext<'_, Data, Error>,
    searching: &str,
) -> Vec<String> {
    fn flatten_commands(
        result: &mut Vec<String>,
        commands: &[poise::Command<Data, Error>],
        searching: &str,
    ) {
        for command in commands {
            if command.owners_only || command.hide_in_help {
                continue;
            }

            if command.subcommands.is_empty() {
                if command.qualified_name.starts_with(searching) {
                    result.push(command.qualified_name.clone());
                }
            // else if command.subcommands == vec!["help"] {
            } else {
                flatten_commands(result, &command.subcommands, searching);
            }
        }
    }

    let commands = &ctx.framework.options().commands;
    let mut result = Vec::with_capacity(commands.len());

    flatten_commands(&mut result, commands, searching);

    result.sort_by_key(|a| strsim::levenshtein(a, searching));
    result
}

// /// Show the help menu.
// #[cfg(not(tarpaulin_include))]
// #[poise::command(category = "Utility", slash_command, prefix_command, track_edits)]
// pub async fn help(
//     ctx: Context<'_>,
//     #[description = "Specific command to show help about"]
//     #[autocomplete = "autocomplete"]
//     #[rest]
//     mut command: Option<String>,
// ) -> Result<(), Error> {
//     // This makes it possible to just make `help` a subcommand of any command
//     // `/fruit help` turns into `/help fruit`
//     // `/fruit help apple` turns into `/help fruit apple`
//     tracing::info!("Invoked command: {}", ctx.invoked_command_name());
//     tracing::info!("Command: {:?}", command);
//     if ctx.invoked_command_name() != "help" {
//         command = match command {
//             Some(c) => Some(format!("{} {}", ctx.invoked_command_name(), c)),
//             None => Some(ctx.invoked_command_name().to_string()),
//         };
//     }
//     poise::builtins::help(
//         ctx,
//         command.as_deref(),
//         poise::builtins::HelpConfiguration {
//             show_context_menu_commands: true,
//             show_subcommands: false,
//             extra_text_at_bottom: "This is a friendly crack smoking parrot that plays music.",
//             ..Default::default()
//         },
//     )
//     .await?;
//     Ok(())
// }
use crate::Command;

#[allow(dead_code)]
enum HelpCommandMode<'a> {
    Root,
    Group(&'a Command),
    Command(&'a Command),
}

#[poise::command(
    prefix_command,
    slash_command,
    required_bot_permissions = "SEND_MESSAGES | EMBED_LINKS"
)]
async fn help(
    ctx: Context<'_>,
    #[rest]
    #[description = "The command to get help with"]
    #[autocomplete = "autocomplete"]
    command: Option<String>,
) -> Result<(), Error> {
    command_func(ctx, command.as_deref()).await
}

pub async fn command_func(ctx: Context<'_>, command: Option<&str>) -> Result<(), Error> {
    let framework_options = ctx.framework().options();
    let commands = &framework_options.commands;

    let remaining_args: String;
    let _mode = match command {
        None => HelpCommandMode::Root,
        Some(command) => {
            let mut subcommand_iterator = command.split(' ');

            let top_level_command = subcommand_iterator.next().unwrap();
            let (mut command_obj, _, _) = require!(
                poise::find_command(commands, top_level_command, true, &mut Vec::new()),
                {
                    let msg = CrackedError::CommandNotFound(top_level_command.to_string());
                    let _ = send_response_poise(ctx, msg.into(), true).await?;
                    Ok(())
                }
            );

            remaining_args = subcommand_iterator.collect();
            if !remaining_args.is_empty() {
                (command_obj, _, _) = require!(
                    poise::find_command(
                        &command_obj.subcommands,
                        &remaining_args,
                        true,
                        &mut Vec::new()
                    ),
                    {
                        let group_name = command_obj.name.clone();
                        let subcommand_name = remaining_args;
                        let msg = CrackedMessage::SubcommandNotFound {
                            group: group_name,
                            subcommand: subcommand_name,
                        };

                        let _ = send_response_poise(ctx, msg, true).await?;
                        Ok(())
                    }
                );
            };

            if command_obj.owners_only && !framework_options.owners.contains(&ctx.author().id) {
                let msg = CrackedMessage::OwnersOnly;
                let _ = send_response_poise(ctx, msg, true).await?;
                return Ok(());
            }

            if command_obj.subcommands.is_empty() {
                HelpCommandMode::Command(command_obj)
            } else {
                HelpCommandMode::Group(command_obj)
            }
        },
    };

    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            show_context_menu_commands: true,
            show_subcommands: false,
            extra_text_at_bottom: "This is a friendly crack smoking parrot that plays music.",
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

//     Ok(())

// let neutral_colour = ctx.neutral_colour().await;
// let embed = CreateEmbed::default()
//     .title(ctx.gettext("{command_name} Help!").replace(
//         "{command_name}",
//         &match &mode {
//             HelpCommandMode::Root => ctx.cache().current_user().name.to_string(),
//             HelpCommandMode::Group(c) | HelpCommandMode::Command(c) => {
//                 format!("`{}`", c.qualified_name)
//             },
//         },
//     ))
//     .description(match &mode {
//         HelpCommandMode::Root => show_group_description(&get_command_mapping(commands)),
//         HelpCommandMode::Command(command_obj) => {
//             let mut msg = format!(
//                 "{}\n```/{}",
//                 command_obj
//                     .description
//                     .as_deref()
//                     .unwrap_or_else(|| ctx.gettext("Command description not found!")),
//                 command_obj.qualified_name
//             );

//             format_params(&mut msg, command_obj);
//             msg.push_str("```\n");

//             if !command_obj.parameters.is_empty() {
//                 msg.push_str(ctx.gettext("__**Parameter Descriptions**__\n"));
//                 command_obj.parameters.iter().for_each(|p| {
//                     let name = &p.name;
//                     let description = p
//                         .description
//                         .as_deref()
//                         .unwrap_or_else(|| ctx.gettext("no description"));
//                     writeln!(msg, "`{name}`: {description}").unwrap();
//                 });
//             };

//             msg
//         },
//         HelpCommandMode::Group(group) => show_group_description(&{
//             let mut map = IndexMap::new();
//             map.insert(
//                 group.qualified_name.as_ref(),
//                 group.subcommands.iter().collect(),
//             );
//             map
//         }),
//     })
//     .colour(neutral_colour)
//     .author(
//         serenity::CreateEmbedAuthor::new(ctx.author().name.as_str())
//             .icon_url(ctx.author().face()),
//     )
//     .footer(serenity::CreateEmbedFooter::new(match mode {
//         HelpCommandMode::Group(c) => Cow::Owned(
//             ctx.gettext("Use `/help {command_name} [command]` for more info on a command")
//                 .replace("{command_name}", &c.qualified_name),
//         ),
//         HelpCommandMode::Command(_) | HelpCommandMode::Root => {
//             Cow::Borrowed(ctx.gettext("Use `/help [command]` for more info on a command"))
//         },
//     }));

// ctx.send(poise::CreateReply::default().embed(embed)).await?;
// Ok(())
//}

// /set calls /help set
pub use command_func as command;
pub fn commands() -> [Command; 1] {
    [help()]
}

// /// Get information about the servers this bot is in.
// #[cfg(not(tarpaulin_include))]
// #[poise::command(slash_command, prefix_command, owners_only, category = "Utility")]
// pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
//     poise::builtins::servers(ctx).await?;
//     Ok(())
// }
