//! Member-facing economy commands: `/credits`, `/coinflip`, `/shop` and `/economy`.

pub mod admin;
pub mod bounty;
pub mod coinflip;
pub mod credits;
pub mod crowdfund;
pub mod shop;

/// Every economy command.
pub fn commands() -> Vec<crate::Command> {
    vec![
        credits::credits(),
        coinflip::coinflip(),
        shop::shop(),
        admin::economy(),
        bounty::bounty(),
        crowdfund::crowdfund(),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn economy_commands_are_guild_only_slash_commands() {
        let cmds = super::commands();
        let names: Vec<_> = cmds.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(
            names,
            [
                "credits",
                "coinflip",
                "shop",
                "economy",
                "bounty",
                "crowdfund"
            ]
        );
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
            "mirror", "cleanse", "reveal", "theme", "dedicate", "jump",
        ];
        want.sort();
        assert_eq!(subs(shop), want);
        assert_eq!(shop.subcommands.len(), 15);
        // The paid plays run the music check, and the game blocks them by
        // their qualified names.
        for name in ["dedicate", "jump"] {
            let sub = shop.subcommands.iter().find(|c| c.name == name).unwrap();
            assert!(!sub.checks.is_empty(), "{name} must carry cmd_check_music");
            let qualified = format!("shop {name}");
            assert!(
                crate::commands::music::gp::GP_BLOCKED_COMMANDS.contains(&qualified.as_str()),
                "{qualified}"
            );
        }
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
        // /bounty places and lists, and place takes the member and the stake.
        let bounty = &cmds[4];
        assert_eq!(subs(bounty), ["list", "place"]);
        assert!(bounty
            .subcommands
            .iter()
            .all(|c| c.guild_only && c.slash_action.is_some()));
        let place = bounty
            .subcommands
            .iter()
            .find(|c| c.name == "place")
            .unwrap();
        let params: Vec<_> = place.parameters.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(params, ["member", "amount", "note", "signed"]);
        // /crowdfund starts, pledges and lists; start's choices are exactly
        // the fundable pranks, and pledge offers its picker.
        let fund = &cmds[5];
        assert_eq!(subs(fund), ["list", "pledge", "start"]);
        assert!(fund
            .subcommands
            .iter()
            .all(|c| c.guild_only && c.slash_action.is_some()));
        let start = fund.subcommands.iter().find(|c| c.name == "start").unwrap();
        let params: Vec<_> = start.parameters.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(
            params,
            ["action", "member", "name", "note", "signed", "pledge"]
        );
        let choices: Vec<_> = start.parameters[0]
            .choices
            .iter()
            .map(|c| c.name.as_ref())
            .collect();
        let want: Vec<_> = crate::economy::settings::ActionKind::FUNDABLE
            .iter()
            .map(|k| k.label())
            .collect();
        assert_eq!(choices, want);
        let pledge = fund
            .subcommands
            .iter()
            .find(|c| c.name == "pledge")
            .unwrap();
        let params: Vec<_> = pledge.parameters.iter().map(|p| p.name.as_ref()).collect();
        assert_eq!(params, ["fund", "amount", "signed"]);
        assert!(pledge.parameters[0].autocomplete_callback.is_some());
    }
}
