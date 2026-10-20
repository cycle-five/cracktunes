//! Member-facing economy commands: `/credits` and `/coinflip`.

pub mod coinflip;
pub mod credits;

/// Every economy command.
pub fn commands() -> Vec<crate::Command> {
    vec![credits::credits(), coinflip::coinflip()]
}

#[cfg(test)]
mod tests {
    #[test]
    fn credits_and_coinflip_are_guild_only_slash_commands() {
        let cmds = super::commands();
        let names: Vec<_> = cmds.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["credits", "coinflip"]);
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
}
