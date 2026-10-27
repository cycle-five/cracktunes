//! FIXME: This is mostly ripped from the poise repo.
//! It would be nice to be able to not maintain these functions for ourselves.
//! Utilities for registering application commands

#![expect(clippy::disallowed_methods, reason = "messaging arc: not migrated yet")]

use super::registration::CommandRegistration;
use crate::messaging::discord;
use crate::messaging::messages::REGISTER_OWNER_ONLY;
use crack_model::{Button, ButtonStyle, Row};
use poise::serenity_prelude as serenity;
use serenity::CollectComponentInteractions;

/// Collects all commands into a [`Vec<serenity::CreateCommand>`] builder, which can be used
/// to register the commands on Discord
///
/// Also see [`register_application_commands_buttons`] for a ready to use register command
///
/// ```rust,no_run
/// # use poise::serenity_prelude as serenity;
/// # async fn foo(ctx: poise::Context<'_, (), ()>) -> Result<(), serenity::Error> {
/// let commands = &ctx.framework().options().commands;
/// let create_commands = poise::builtins::create_application_commands(commands);
///
/// serenity::Command::set_global_commands(ctx.http(), &create_commands).await?;
/// # Ok(()) }
/// ```
pub fn create_application_commands_cracked<U, E>(
    commands: &[poise::Command<U, E>],
) -> Vec<serenity::CreateCommand<'static>> {
    create_application_commands_filtered(commands, |_| true)
}

/// Like [`create_application_commands_cracked`], but builds only the top-level
/// commands `keep` accepts (a kept command brings its context menus along).
pub(crate) fn create_application_commands_filtered<U, E>(
    commands: &[poise::Command<U, E>],
    keep: impl Fn(&poise::Command<U, E>) -> bool,
) -> Vec<serenity::CreateCommand<'static>> {
    /// We decided to extract context menu commands recursively, despite the subcommand hierarchy
    /// not being preserved. Because it's more confusing to just silently discard context menu
    /// commands if they're not top-level commands.
    /// https://discord.com/channels/381880193251409931/919310428344029265/947970605985189989
    fn recursively_add_context_menu_commands<U, E>(
        builder: &mut Vec<serenity::CreateCommand<'static>>,
        command: &poise::Command<U, E>,
    ) {
        if let Some(context_menu_command) = command.create_as_context_menu_command() {
            builder.push(context_menu_command);
        }
        for subcommand in &command.subcommands {
            if subcommand.name != "help" {
                recursively_add_context_menu_commands(builder, subcommand);
            }
        }
    }

    let mut commands_builder = Vec::with_capacity(commands.len());
    for command in commands.iter().filter(|command| keep(command)) {
        if let Some(slash_command) = command.create_as_slash_command() {
            commands_builder.push(slash_command);
        }
        recursively_add_context_menu_commands(&mut commands_builder, command);
    }
    commands_builder
}

/// One of the four choices `/register` can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterButton {
    RegisterGuild,
    UnregisterGuild,
    RegisterGlobal,
    UnregisterGlobal,
}

impl RegisterButton {
    /// The component custom id, which is what comes back on a press.
    pub fn custom_id(self) -> &'static str {
        match self {
            Self::RegisterGuild => "register.guild",
            Self::UnregisterGuild => "unregister.guild",
            Self::RegisterGlobal => "register.global",
            Self::UnregisterGlobal => "unregister.global",
        }
    }

    pub fn from_custom_id(id: &str) -> Option<Self> {
        [
            Self::RegisterGuild,
            Self::UnregisterGuild,
            Self::RegisterGlobal,
            Self::UnregisterGlobal,
        ]
        .into_iter()
        .find(|b| b.custom_id() == id)
    }

    pub fn button(self) -> Button {
        let (label, style, emoji) = match self {
            Self::RegisterGuild => ("Register in guild", ButtonStyle::Primary, "📋"),
            Self::UnregisterGuild => ("Delete in guild", ButtonStyle::Danger, "🗑"),
            Self::RegisterGlobal => ("Register globally", ButtonStyle::Primary, "📋"),
            Self::UnregisterGlobal => ("Unregister globally", ButtonStyle::Danger, "🗑"),
        };
        Button {
            id: self.custom_id().to_owned(),
            label: Some(label.to_owned()),
            emoji: Some(emoji.to_owned()),
            style,
            disabled: false,
        }
    }
}

/// The one row `/register` shows its buttons in.
fn button_rows(offered: &[RegisterButton]) -> Vec<Row> {
    vec![Row::Buttons(offered.iter().map(|b| b.button()).collect())]
}

/// The buttons `/register` offers. The bot owner gets all four. In `Manual`
/// mode anyone else who can run the command (mods) may register or delete
/// their own server's commands; in `Global` mode only the owner may do anything.
pub fn offered_buttons(mode: CommandRegistration, is_owner: bool) -> Vec<RegisterButton> {
    use RegisterButton::{RegisterGlobal, RegisterGuild, UnregisterGlobal, UnregisterGuild};
    match (mode, is_owner) {
        (_, true) => vec![
            RegisterGuild,
            UnregisterGuild,
            RegisterGlobal,
            UnregisterGlobal,
        ],
        (CommandRegistration::Manual, false) => vec![RegisterGuild, UnregisterGuild],
        (CommandRegistration::Global, false) => vec![],
    }
}

/// What a pressed component means, given the buttons that were offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Not one of `/register`'s buttons.
    Unknown,
    /// A real button, but not one this caller was offered.
    Refused,
    Act(RegisterButton),
}

/// Decide what a press does. Only buttons in `offered` may act.
pub fn resolve_press(offered: &[RegisterButton], custom_id: &str) -> Press {
    match RegisterButton::from_custom_id(custom_id) {
        None => Press::Unknown,
        Some(button) if offered.contains(&button) => Press::Act(button),
        Some(_) => Press::Refused,
    }
}

/// Offers the buttons [`offered_buttons`] allows for this mode and caller, to register or delete
/// application commands globally or in the current guild
///
/// Upgraded version of [`register_application_commands`]
///
/// ![Screenshot of output](https://imgur.com/rTbTaDs.png)
///
/// You probably want to use this by wrapping it in a small `register` command:
/// ```rust
/// # type Error = Box<dyn std::error::Error + Send + Sync>;
/// # type Context<'a> = poise::Context<'a, (), Error>;
/// #[poise::command(prefix_command)]
/// pub async fn register(ctx: Context<'_>) -> Result<(), Error> {
///     poise::builtins::register_application_commands_buttons(ctx).await?;
///     Ok(())
/// }
///
/// // ...
/// poise::FrameworkOptions {
///     commands: vec![
///         // ...
///         register(),
///     ],
/// #   ..Default::default()
/// };
/// ```
///
/// Which you can call like any prefix command, for example `@your_bot register`.
///
/// Only the buttons [`offered_buttons`] allows are shown, and only those may be pressed.
pub async fn register_application_commands_buttons_cracked<U: Sync + Send + 'static, E>(
    ctx: poise::Context<'_, U, E>,
    mode: CommandRegistration,
) -> Result<(), serenity::Error> {
    let is_bot_owner = ctx.framework().options().owners.contains(&ctx.author().id);
    let offered = offered_buttons(mode, is_bot_owner);
    if offered.is_empty() {
        ctx.say(REGISTER_OWNER_ONLY).await?;
        return Ok(());
    }

    let builder = poise::CreateReply::default()
        .content("Choose what to do with the commands:")
        .components(discord::action_rows(&button_rows(&offered)));

    let reply = ctx.send(builder).await?;

    let interaction = reply
        .message()
        .await?
        .into_owned()
        .id
        .collect_component_interactions(ctx.serenity_context())
        .author_id(ctx.author().id)
        .await;

    reply
        .edit(
            ctx,
            poise::CreateReply::default()
                .components(vec![])
                .content("Processing... Please wait."),
        )
        .await?; // remove buttons after button press and edit message
    let pressed_button_id = match &interaction {
        Some(m) => &m.data.custom_id,
        None => {
            ctx.say(":warning: You didn't interact in time - please run the command again.")
                .await?;
            return Ok(());
        },
    };

    let pressed = match resolve_press(&offered, pressed_button_id.as_str()) {
        Press::Unknown => {
            tracing::warn!("unknown register button ID: {:?}", pressed_button_id);
            return Ok(());
        },
        Press::Refused => {
            ctx.say(REGISTER_OWNER_ONLY).await?;
            return Ok(());
        },
        Press::Act(button) => button,
    };
    let (register, global) = match pressed {
        RegisterButton::RegisterGlobal => (true, true),
        RegisterButton::UnregisterGlobal => (false, true),
        RegisterButton::RegisterGuild => (true, false),
        RegisterButton::UnregisterGuild => (false, false),
    };

    let start_time = std::time::Instant::now();

    if global {
        let create_commands =
            create_application_commands_cracked(&ctx.framework().options().commands);
        if register {
            ctx.say(format!(
                ":gear: Registering {} global commands...",
                create_commands.len(),
            ))
            .await?;
            serenity::Command::set_global_commands(ctx.http(), create_commands.as_slice()).await?;
        } else {
            ctx.say(":gear: Unregistering global commands...").await?;
            serenity::Command::set_global_commands(ctx.http(), vec![].as_slice()).await?;
        }
    } else {
        let guild_id = match ctx.guild_id() {
            Some(x) => x,
            None => {
                ctx.say(":x: Must be called in guild").await?;
                return Ok(());
            },
        };
        if register {
            let guild_commands = mode.guild_commands(&ctx.framework().options().commands);
            ctx.say(format!(
                ":gear: Registering {} guild commands...",
                guild_commands.len(),
            ))
            .await?;
            guild_id
                .set_commands(ctx.http(), guild_commands.as_slice())
                .await?;
        } else {
            ctx.say(":gear: Unregistering guild commands...").await?;
            guild_id.set_commands(ctx.http(), vec![].as_slice()).await?;
        }
    }

    // Calulate time taken and send message
    let time_taken = start_time.elapsed();
    ctx.say(format!(
        ":white_check_mark: Done! Took {}ms",
        time_taken.as_millis()
    ))
    .await?;

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::commands::registration::CommandRegistration::{Global, Manual};
    use RegisterButton::*;

    #[test]
    fn offered_buttons_by_mode_and_owner() {
        let all = vec![
            RegisterGuild,
            UnregisterGuild,
            RegisterGlobal,
            UnregisterGlobal,
        ];
        assert_eq!(offered_buttons(Global, true), all);
        assert_eq!(offered_buttons(Manual, true), all);
        assert_eq!(
            offered_buttons(Manual, false),
            vec![RegisterGuild, UnregisterGuild]
        );
        assert_eq!(offered_buttons(Global, false), vec![]);
    }

    #[test]
    fn the_register_buttons_render_as_before() {
        let row = discord::action_rows(&button_rows(&[
            RegisterGuild,
            UnregisterGuild,
            RegisterGlobal,
            UnregisterGlobal,
        ]));
        assert_eq!(
            serde_json::to_string(&row).unwrap(),
            r#"[{"type":1,"components":[{"style":1,"type":2,"custom_id":"register.guild","label":"Register in guild","emoji":{"name":"📋"},"disabled":false},{"style":4,"type":2,"custom_id":"unregister.guild","label":"Delete in guild","emoji":{"name":"🗑"},"disabled":false},{"style":1,"type":2,"custom_id":"register.global","label":"Register globally","emoji":{"name":"📋"},"disabled":false},{"style":4,"type":2,"custom_id":"unregister.global","label":"Unregister globally","emoji":{"name":"🗑"},"disabled":false}]}]"#
        );
    }

    #[test]
    fn custom_ids_round_trip() {
        for b in [
            RegisterGuild,
            UnregisterGuild,
            RegisterGlobal,
            UnregisterGlobal,
        ] {
            assert_eq!(RegisterButton::from_custom_id(b.custom_id()), Some(b));
        }
        assert_eq!(RegisterButton::from_custom_id("nope"), None);
    }

    #[test]
    fn a_press_that_was_not_offered_is_refused() {
        let mods = offered_buttons(Manual, false);
        assert_eq!(resolve_press(&mods, "register.global"), Press::Refused);
        assert_eq!(resolve_press(&mods, "unregister.global"), Press::Refused);
        assert_eq!(
            resolve_press(&mods, "register.guild"),
            Press::Act(RegisterGuild)
        );
        assert_eq!(
            resolve_press(&offered_buttons(Global, true), "nope"),
            Press::Unknown
        );
    }
}
