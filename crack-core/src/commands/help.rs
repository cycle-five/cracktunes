use crate::commands::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::EXTRA_TEXT_AT_BOTTOM;
use crate::utils::{create_paged_embed, send_reply};
use crate::{require, Context, Data, Error};
use itertools::Itertools;

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

/// Show the help menu.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Utility",
    rename = "help",
    //slash_command, FIXME: Can we have this work with the slash commands?
    prefix_command,
    hide_in_help
)]
pub async fn sub_help(ctx: Context<'_>) -> Result<(), Error> {
    // This makes it possible to just make `help` a subcommand of any command
    // `/fruit help` turns into `/help fruit`
    // `/fruit help apple` turns into `/help fruit apple`
    let parent = ctx
        .parent_commands()
        .iter()
        .map(|&x| x.name.clone())
        .join(" ");
    command_func(ctx, Some(&parent)).await
}

use crate::Command;
#[allow(dead_code)]
enum HelpCommandMode<'a> {
    Root,
    Group(&'a Command),
    Command(&'a Command),
}

/// Shows the help menu.
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

/// Wrapper around the help function.
pub async fn wrapper(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::help(
        ctx,
        Some(ctx.command().name.as_str()),
        poise::builtins::HelpConfiguration {
            show_context_menu_commands: false,
            show_subcommands: false,
            extra_text_at_bottom: EXTRA_TEXT_AT_BOTTOM,
            include_description: false,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
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
                    let _ = send_reply(&ctx, msg.into(), true).await?;
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

                        let _ = send_reply(&ctx, msg, true).await?;
                        Ok(())
                    }
                );
            };

            if command_obj.owners_only && !framework_options.owners.contains(&ctx.author().id) {
                let msg = CrackedMessage::OwnersOnly;
                let _ = send_reply(&ctx, msg, true).await?;
                return Ok(());
            }

            if command_obj.subcommands.is_empty() {
                HelpCommandMode::Command(command_obj)
            } else {
                HelpCommandMode::Group(command_obj)
            }
        },
    };

    // // let neutral_colour = Colour::from_rgb(0x00, 0x00, 0x00);
    // let neutral_colour = Colour::BLURPLE;
    // let embed = CreateEmbed::default()
    //     .title("{command_name} Help!".to_string().replace(
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
    //                     .unwrap_or_else(|| "Command description not found!"),
    //                 command_obj.qualified_name
    //             );

    //             format_params(&mut msg, command_obj);
    //             msg.push_str("```\n");

    //             if !command_obj.parameters.is_empty() {
    //                 msg.push_str("__**Parameter Descriptions**__\n");
    //                 command_obj.parameters.iter().for_each(|p| {
    //                     let name = &p.name;
    //                     let description =
    //                         p.description.as_deref().unwrap_or_else(|| "no description");
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

    poise::builtins::help(
        ctx,
        command,
        poise::builtins::HelpConfiguration {
            show_context_menu_commands: false,
            show_subcommands: false,
            extra_text_at_bottom: EXTRA_TEXT_AT_BOTTOM,
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
pub fn help_commands() -> [Command; 1] {
    [help()]
}

// Contains the built-in help command and surrounding infrastructure

use poise::serenity_prelude as serenity;
use std::fmt::Write as _;

/// Optional configuration for how the help message from [`help()`] looks
pub struct HelpConfiguration<'a> {
    /// Extra text displayed at the bottom of your message. Can be used for help and tips specific
    /// to your bot
    pub extra_text_at_bottom: &'a str,
    /// Whether to make the response ephemeral if possible. Can be nice to reduce clutter
    pub ephemeral: bool,
    /// Whether to list context menu commands as well
    pub show_context_menu_commands: bool,
    /// Whether to list context menu commands as well
    pub show_subcommands: bool,
    /// Whether to include [`crate::Command::description`] (above [`crate::Command::help_text`]).
    pub include_description: bool,
    #[doc(hidden)]
    pub __non_exhaustive: (),
}

impl Default for HelpConfiguration<'_> {
    fn default() -> Self {
        Self {
            extra_text_at_bottom: "",
            ephemeral: true,
            show_context_menu_commands: false,
            show_subcommands: false,
            include_description: true,
            __non_exhaustive: (),
        }
    }
}

/// Convenience function to align descriptions behind commands
struct TwoColumnList(Vec<(String, Option<String>)>);

impl TwoColumnList {
    /// Creates a new [`TwoColumnList`]
    fn new() -> Self {
        Self(Vec::new())
    }

    /// Add a line that needs the padding between the columns
    fn push_two_colums(&mut self, command: String, description: String) {
        self.0.push((command, Some(description)));
    }

    /// Add a line that doesn't influence the first columns's width
    fn push_heading(&mut self, category: &str) {
        if !self.0.is_empty() {
            self.0.push(("".to_string(), None));
        }
        let mut category = category.to_string();
        category += ":";
        self.0.push((category, None));
    }

    /// Convert the list into a string with aligned descriptions
    fn into_string(self) -> String {
        let longest_command = self
            .0
            .iter()
            .filter_map(|(command, description)| {
                if description.is_some() {
                    Some(command.len())
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(0);
        let mut text = String::new();
        for (command, description) in self.0 {
            if let Some(description) = description {
                let padding = " ".repeat(longest_command - command.len() + 3);
                writeln!(text, "{}{}{}", command, padding, description).unwrap();
            } else {
                writeln!(text, "{}", command).unwrap();
            }
        }
        text
    }
}

/// Get the prefix from options
pub(super) async fn get_prefix_from_options<U, E>(ctx: poise::Context<'_, U, E>) -> Option<String> {
    let options = &ctx.framework().options().prefix_options;
    match &options.prefix {
        Some(fixed_prefix) => Some(fixed_prefix.clone()),
        None => match options.dynamic_prefix {
            Some(dynamic_prefix_callback) => {
                match dynamic_prefix_callback(poise::PartialContext::from(ctx)).await {
                    Ok(Some(dynamic_prefix)) => Some(dynamic_prefix),
                    _ => None,
                }
            },
            None => None,
        },
    }
}

/// Format context menu command name
fn format_context_menu_name(command: &crate::Command) -> Option<String> {
    let kind = match command.context_menu_action {
        Some(poise::ContextMenuCommandAction::User(_)) => "user",
        Some(poise::ContextMenuCommandAction::Message(_)) => "message",
        Some(poise::ContextMenuCommandAction::__NonExhaustive) => unreachable!(),
        None => return None,
    };
    Some(format!(
        "{} (on {})",
        command
            .context_menu_name
            .as_deref()
            .unwrap_or(&command.name),
        kind
    ))
}

/// Code for printing help of a specific command (e.g. `~help my_command`)
async fn help_single_command(
    ctx: crate::Context<'_>,
    command_name: &str,
    config: HelpConfiguration<'_>,
) -> Result<(), serenity::Error> {
    let commands = &ctx.framework().options().commands;
    // Try interpret the command name as a context menu command first
    let mut command = commands.iter().find(|command| {
        if let Some(context_menu_name) = &command.context_menu_name {
            if context_menu_name.eq_ignore_ascii_case(command_name) {
                return true;
            }
        }
        false
    });
    // Then interpret command name as a normal command (possibly nested subcommand)
    if command.is_none() {
        if let Some((c, _, _)) = poise::find_command(commands, command_name, true, &mut vec![]) {
            command = Some(c);
        }
    }

    let reply = if let Some(command) = command {
        let mut invocations = Vec::new();
        let mut subprefix = None;
        if command.slash_action.is_some() {
            invocations.push(format!("`/{}`", command.name));
            subprefix = Some(format!("  /{}", command.name));
        }
        if command.prefix_action.is_some() {
            let prefix = match get_prefix_from_options(ctx).await {
                Some(prefix) => prefix,
                // None can happen if the prefix is dynamic, and the callback
                // fails due to help being invoked with slash or context menu
                // commands. Not sure there's a better way to handle this.
                None => String::from("<prefix>"),
            };
            invocations.push(format!("`{}{}`", prefix, command.name));
            if subprefix.is_none() {
                subprefix = Some(format!("  {}{}", prefix, command.name));
            }
        }
        if command.context_menu_name.is_some() && command.context_menu_action.is_some() {
            // Since command.context_menu_action is Some, this unwrap is safe
            invocations.push(format_context_menu_name(command).unwrap());
            if subprefix.is_none() {
                subprefix = Some(String::from("  "));
            }
        }
        // At least one of the three if blocks should have triggered
        assert!(subprefix.is_some());
        assert!(!invocations.is_empty());
        let invocations = invocations.join("\n");

        let mut text = match (&command.description, &command.help_text) {
            (Some(description), Some(help_text)) => {
                if config.include_description {
                    format!("{}\n\n{}", description, help_text)
                } else {
                    help_text.clone()
                }
            },
            (Some(description), None) => description.to_owned(),
            (None, Some(help_text)) => help_text.clone(),
            (None, None) => "No help available".to_string(),
        };
        if !command.parameters.is_empty() {
            text += "\n\n```\nParameters:\n";
            let mut parameterlist = TwoColumnList::new();
            for parameter in &command.parameters {
                let name = parameter.name.clone();
                let description = parameter.description.as_deref().unwrap_or("");
                let description = format!(
                    "({}) {}",
                    if parameter.required {
                        "required"
                    } else {
                        "optional"
                    },
                    description,
                );
                parameterlist.push_two_colums(name, description);
            }
            text += &parameterlist.into_string();
            text += "```";
        }
        if !command.subcommands.is_empty() {
            text += "\n\n```\nSubcommands:\n";
            let mut commandlist = TwoColumnList::new();
            // Subcommands can exist on context menu commands, but there's no
            // hierarchy in the menu, so just display them as a list without
            // subprefix.
            preformat_subcommands(
                &mut commandlist,
                command,
                &subprefix.unwrap_or_else(|| String::from("  ")),
            );
            text += &commandlist.into_string();
            text += "```";
        }
        format!("**{}**\n\n{}", invocations, text)
    } else {
        format!("No such command `{}`", command_name)
    };

    // let reply = CreateReply::default()
    //     .content(reply)
    //     .ephemeral(config.ephemeral);

    create_paged_embed(
        ctx,
        "Help".to_string(),
        "Help".to_string(),
        reply,
        1024, //ctx.data().bot_settings.lyrics_page_size,
    )
    .await?;

    // ctx.send(reply).await?;
    Ok(())
}

/// Recursively formats all subcommands
fn preformat_subcommands(commands: &mut TwoColumnList, command: &crate::Command, prefix: &str) {
    let as_context_command = command.slash_action.is_none() && command.prefix_action.is_none();
    for subcommand in &command.subcommands {
        let command = if as_context_command {
            let name = format_context_menu_name(subcommand);
            if name.is_none() {
                continue;
            };
            name.unwrap()
        } else {
            format!("{} {}", prefix, subcommand.name)
        };
        let description = subcommand.description.as_deref().unwrap_or("").to_string();
        commands.push_two_colums(command, description);
        // We could recurse here, but things can get cluttered quickly.
        // Instead, we show (using this function) subsubcommands when
        // the user asks for help on the subcommand.
    }
}

/// Preformat lines (except for padding,) like `("  /ping", "Emits a ping message")`
fn preformat_command(
    commands: &mut TwoColumnList,
    config: &HelpConfiguration<'_>,
    command: &crate::Command,
    indent: &str,
    options_prefix: Option<&str>,
) {
    let prefix = if command.slash_action.is_some() {
        String::from("/")
    } else if command.prefix_action.is_some() {
        options_prefix.map(String::from).unwrap_or_default()
    } else {
        // This is not a prefix or slash command, i.e. probably a context menu only command
        // This should have been filtered out in `generate_all_commands`
        unreachable!();
    };

    let prefix = format!("{}{}{}", indent, prefix, command.name);
    commands.push_two_colums(
        prefix.clone(),
        command.description.as_deref().unwrap_or("").to_string(),
    );
    if config.show_subcommands {
        preformat_subcommands(commands, command, &prefix)
    }
}

/// Create help text for `help_all_commands`
///
/// This is a separate function so we can have tests for it
async fn generate_all_commands(
    ctx: crate::Context<'_>,
    config: &HelpConfiguration<'_>,
) -> Result<String, serenity::Error> {
    let mut categories = indexmap::IndexMap::<Option<&str>, Vec<&crate::Command>>::new();
    for cmd in &ctx.framework().options().commands {
        categories
            .entry(cmd.category.as_deref())
            .or_default()
            .push(cmd);
    }

    let options_prefix = get_prefix_from_options(ctx).await;

    let mut menu = String::from("```\n");

    let mut commandlist = TwoColumnList::new();
    for (category_name, commands) in categories {
        let commands = commands
            .into_iter()
            .filter(|cmd| {
                !cmd.hide_in_help && (cmd.prefix_action.is_some() || cmd.slash_action.is_some())
            })
            .collect::<Vec<_>>();
        if commands.is_empty() {
            continue;
        }
        commandlist.push_heading(category_name.unwrap_or("Commands"));
        for command in commands {
            preformat_command(
                &mut commandlist,
                config,
                command,
                "  ",
                options_prefix.as_deref(),
            );
        }
    }
    menu += &commandlist.into_string();

    if config.show_context_menu_commands {
        menu += "\nContext menu commands:\n";

        for command in &ctx.framework().options().commands {
            let name = format_context_menu_name(command);
            if name.is_none() {
                continue;
            };
            let _ = writeln!(menu, "  {}", name.unwrap());
        }
    }

    menu += "\n";
    menu += config.extra_text_at_bottom;
    menu += "\n```";

    Ok(menu)
}

/// Code for printing an overview of all commands (e.g. `~help`)
async fn help_all_commands(
    ctx: crate::Context<'_>,
    config: HelpConfiguration<'_>,
) -> Result<(), serenity::Error> {
    let menu = generate_all_commands(ctx, &config).await?;
    // let reply = CreateReply::default()
    //     .content(menu)
    //     .ephemeral(config.ephemeral);

    // ctx.send(reply).await?;
    create_paged_embed(ctx, "Help".to_string(), "Help".to_string(), menu, 1024).await?;
    Ok(())
}

/// A help command that outputs text in a code block, groups commands by categories, and annotates
/// commands with a slash if they exist as slash commands.
///
/// Example usage from Ferris, the Discord bot running in the Rust community server:
/// ```rust
/// # type Error = Box<dyn std::error::Error>;
/// # type Context<'a> = poise::Context<'a, (), Error>;
/// /// Show this menu
/// #[poise::command(prefix_command, track_edits, slash_command)]
/// pub async fn help(
///     ctx: Context<'_>,
///     #[description = "Specific command to show help about"] command: Option<String>,
/// ) -> Result<(), Error> {
///     let config = poise::builtins::HelpConfiguration {
///         extra_text_at_bottom: "\
/// Type ?help command for more info on a command.
/// You can edit your message to the bot and the bot will edit its response.",
///         ..Default::default()
///     };
///     poise::builtins::help(ctx, command.as_deref(), config).await?;
///     Ok(())
/// }
/// ```
/// Output:
/// ```text
/// Playground:
///   ?play        Compile and run Rust code in a playground
///   ?eval        Evaluate a single Rust expression
///   ?miri        Run code and detect undefined behavior using Miri
///   ?expand      Expand macros to their raw desugared form
///   ?clippy      Catch common mistakes using the Clippy linter
///   ?fmt         Format code using rustfmt
///   ?microbench  Benchmark small snippets of code
///   ?procmacro   Compile and use a procedural macro
///   ?godbolt     View assembly using Godbolt
///   ?mca         Run performance analysis using llvm-mca
///   ?llvmir      View LLVM IR using Godbolt
/// Crates:
///   /crate       Lookup crates on crates.io
///   /doc         Lookup documentation
/// Moderation:
///   /cleanup     Deletes the bot's messages for cleanup
///   /ban         Bans another person
///   ?move        Move a discussion to another channel
///   /rustify     Adds the Rustacean role to members
/// Miscellaneous:
///   ?go          Evaluates Go code
///   /source      Links to the bot GitHub repo
///   /help        Show this menu
///
/// Type ?help command for more info on a command.
/// You can edit your message to the bot and the bot will edit its response.
/// ```
pub async fn builtin_help(
    ctx: crate::Context<'_>,
    command: Option<&str>,
    config: HelpConfiguration<'_>,
) -> Result<(), serenity::Error> {
    match command {
        Some(command) => help_single_command(ctx, command, config).await,
        None => help_all_commands(ctx, config).await,
    }
}
