//! Member-facing economy commands: `/credits`, `/coinflip`, `/shop` and `/economy`.

pub mod admin;
pub mod coinflip;
pub mod credits;
pub mod shop;

/// Every economy command.
pub fn commands() -> Vec<crate::Command> {
    vec![
        credits::credits(),
        coinflip::coinflip(),
        shop::shop(),
        admin::economy(),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn economy_commands_are_guild_only_slash_commands() {
        let cmds = super::commands();
        let names: Vec<_> = cmds.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["credits", "coinflip", "shop", "economy"]);
        assert!(cmds
            .iter()
            .all(|c| c.guild_only && c.slash_action.is_some()));
        let credits = &cmds[0];
        let mut subs: Vec<_> = credits
            .subcommands
            .iter()
            .map(|s| s.name.as_ref())
            .collect();
        subs.sort();
        assert_eq!(
            subs,
            ["balance", "gift", "history", "optin", "optout", "top"]
        );
    }

    fn subs(c: &crate::Command) -> Vec<&str> {
        let mut subs: Vec<_> = c.subcommands.iter().map(|s| s.name.as_ref()).collect();
        subs.sort();
        subs
    }

    #[test]
    fn commands_registry_has_the_new_subcommands() {
        let cmds = super::commands();
        let shop = &cmds[2];
        let mut want = [
            "menu", "immunity", "rename", "lock", "clown", "drag", "mute", "deafen", "jail",
            "mirror", "cleanse", "reveal",
        ];
        want.sort();
        assert_eq!(subs(shop), want);
        assert_eq!(shop.subcommands.len(), 12);
        let economy = &cmds[3];
        assert_eq!(
            subs(economy),
            ["adjust", "cancel", "grant", "lift", "status"]
        );
        // Every one is a guild-only slash command.
        assert!(shop
            .subcommands
            .iter()
            .chain(&economy.subcommands)
            .all(|c| c.guild_only && c.slash_action.is_some()));
        // Reveal offers its picker.
        let reveal = shop
            .subcommands
            .iter()
            .find(|c| c.name == "reveal")
            .unwrap();
        assert!(reveal
            .parameters
            .iter()
            .all(|p| p.autocomplete_callback.is_some()));
        assert_eq!(reveal.parameters.len(), 1);
    }
}
