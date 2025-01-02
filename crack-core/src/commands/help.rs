//----------------------------------------------------
//! Contains the help command and related functions.
//! This file contains much code originally written by gnomeddev
//! for the poise crate. I've adapted it to work here now that it
//! was removed from poise.
//! cycle.five
//---------------------------------------------------

use crate::commands::CrackedError;
use crate::messaging::message::CrackedMessage;
use crate::utils::{create_paged_embed, send_reply_owned};
use crate::{require, Context, Data, Error};
use ::serenity::all::{AutocompleteChoice, CreateAutocompleteResponse};
use crack_types::messaging::messages::EXTRA_TEXT_AT_BOTTOM;
use itertools::Itertools;

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

#[allow(clippy::unused_async)]
pub async fn autocomplete<'a>(
    ctx: poise::ApplicationContext<'_, Data, Error>,
    searching: &str,
) -> CreateAutocompleteResponse<'a> {
    let commands = ctx.framework.options().commands.as_slice();
    // let choices: &[AutocompleteChoice<'a>] = autocomplete(commands, searching)
    let choices = get_matching_commands(commands, searching)
        .await
        .unwrap_or_default();
    CreateAutocompleteResponse::new().set_choices(Cow::Owned(choices))
}

/// Gets takes the given str and returns the top matching autocomplete choices.
#[allow(clippy::unused_async)]
pub async fn get_matching_commands(
    //ctx: poise::ApplicationContext<'_, Data, Error>,
    commands: &[poise::Command<Data, Error>],
    searching: &str,
) -> Result<Vec<AutocompleteChoice<'static>>, Error> {
    let result = get_matching_command_strs(commands, searching).await?;

    let result: Vec<AutocompleteChoice<'static>> = result
        .into_iter()
        .map(|command| AutocompleteChoice {
            name: command.clone(),
            name_localizations: None,
            value: serenity::AutocompleteValue::String(command),
        })
        .collect::<Vec<AutocompleteChoice<'static>>>();
    Ok(result)
}

/// Get the matching commands for the searching string.
/// N.B. This allocates memory for the results.
///      Results are sorted by the levenshtein distance from the searching string.
/// TODO: Is there a way to avoid the 'static return lifetime?
pub async fn get_matching_command_strs(
    commands: &[poise::Command<Data, Error>],
    searching: &str,
) -> Result<Vec<Cow<'static, str>>, Error> {
    /// Flatten the commands list into the full qualified names of the commands,
    /// which start with the searching string. This means all the the subcommands
    /// are included for each command, unless they are tagged as owners_only or
    /// hide_in_help.
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
                    result.push(command.qualified_name.to_string());
                }
            } else {
                flatten_commands(result, &command.subcommands, searching);
            }
        }
    }

    let mut result = Vec::with_capacity(commands.len());

    flatten_commands(&mut result, commands, searching);

    let mut result = result
        .into_iter()
        .map(|s| Cow::Owned(s.to_owned()))
        .collect::<Vec<_>>();
    result.sort_by_key(|a| strsim::levenshtein(a, searching));
    Ok(result)
}

/// The help function from builtins copied.
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

/// Show the help menu.
#[cfg(not(tarpaulin_include))]
#[poise::command(category = "Utility", rename = "help", prefix_command, hide_in_help)]
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
    builtin_help(
        ctx,
        Some(ctx.command().name.as_ref()),
        HelpConfiguration {
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

    if command.is_none() {
        builtin_help(
            ctx,
            None,
            HelpConfiguration {
                show_context_menu_commands: false,
                show_subcommands: false,
                extra_text_at_bottom: EXTRA_TEXT_AT_BOTTOM,
                ..Default::default()
            },
        )
        .await?;
        return Ok(());
        // Mode is Root
        // None => HelpCommandMode::Root,
    }
    let mut cmd_str: String = command.unwrap().to_owned();
    // We just checked that command is not None, so this unwrap is safe
    let (mut command_obj, remaining_args) = {
        let n = cmd_str.find(' ').unwrap_or(cmd_str.len());
        let remainder = cmd_str.split_off(n);

        let opt_find_cmd = poise::find_command(commands, &cmd_str, true, &mut Vec::new());
        let command_obj = match opt_find_cmd {
            Some((cmd, _, _)) => cmd,
            None => {
                let msg = CrackedError::CommandNotFound(Cow::Owned(cmd_str.clone()));
                let _ = send_reply_owned(ctx, msg.into(), true).await?;
                return Ok(());
            },
        };

        (command_obj, remainder)
    };

    if !remaining_args.is_empty() {
        //let mut command_obj_copy = command_obj.clone();
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
                    group: Cow::Owned(group_name.to_string().clone()),
                    subcommand: Cow::Owned(subcommand_name.to_string().clone()),
                };

                let _ = send_reply_owned(ctx, msg, true).await?;
                Ok(())
            }
        );
    };

    if command_obj.owners_only && !framework_options.owners.contains(&ctx.author().id) {
        let msg = CrackedMessage::OwnersOnly;
        let _ = send_reply_owned(ctx, msg, true).await?;
        return Ok(());
    }

    // if command_obj.subcommands.is_empty() {
    //     HelpCommandMode::Command(command_obj)
    // } else {
    //     HelpCommandMode::Group(command_obj)
    // }

    builtin_help(
        ctx,
        command,
        HelpConfiguration {
            show_context_menu_commands: false,
            show_subcommands: false,
            extra_text_at_bottom: EXTRA_TEXT_AT_BOTTOM,
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

// /set calls /help set
pub use command_func as command;
pub fn help_commands() -> [Command; 1] {
    [help()]
}

// Contains the built-in help command and surrounding infrastructure

use poise::{serenity_prelude as serenity, CreateReply};
use std::borrow::Cow;
use std::fmt::Write as _;

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
            //let command = command.replace("_", r#"\\_"#);
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
pub(super) async fn get_prefix_from_options<U: Sync + Send + 'static, E>(
    ctx: poise::Context<'_, U, E>,
) -> Option<String> {
    let options = &ctx.framework().options().prefix_options;
    match &options.prefix {
        Some(fixed_prefix) => Some(fixed_prefix.to_string()),
        None => match options.dynamic_prefix {
            Some(dynamic_prefix_callback) => {
                match dynamic_prefix_callback(poise::PartialContext::from(ctx)).await {
                    Ok(Some(dynamic_prefix)) => Some(dynamic_prefix.to_string()),
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

        let mut text: String = match (&command.description, &command.help_text) {
            (Some(description), Some(help_text)) => {
                if config.include_description {
                    //Cow::Borrowed(format!("{}\n\n{}", description, help_text).as_str())
                    format!("{}\n\n{}", description, help_text).to_owned()
                } else {
                    help_text.to_string()
                }
            },
            (Some(description), None) => description.to_string(),
            (None, Some(help_text)) => help_text.to_string(),
            (None, None) => String::from("No help available"),
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
                parameterlist.push_two_colums(name.to_string(), description);
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

    if reply.len() > 1000 {
        let bot_name = ctx.cache().current_user().name.clone();
        create_paged_embed(ctx, bot_name, "Help".to_string(), reply, 900).await?;
    } else {
        let create_reply = CreateReply::default()
            .content(reply)
            .ephemeral(config.ephemeral);
        ctx.send(create_reply).await?;
    }

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

    //let mut menu = String::from("```\n");
    let mut menu = String::from("");

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
    //menu += "\n```";

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
    let bot_name = ctx.cache().current_user().name.clone();
    create_paged_embed(ctx, bot_name, "Help".to_string(), menu, 900).await?;
    Ok(())
}
