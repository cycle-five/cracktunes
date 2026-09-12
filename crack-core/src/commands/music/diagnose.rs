//! `/diagnose` — report what the bot can and cannot do here.
//!
//! Deliberately carries no `check`. Every other music command has
//! `check = "cmd_check_music"`, which refuses invocation outside the
//! configured music channel; a diagnostic that can itself be refused for a
//! configuration reason is the worst possible diagnostic, because the reason
//! to run it is that the bot is being silent.

use crate::music::perms::{resolve, MusicPermissions};
use crate::{Context, Error};
use poise::serenity_prelude::all::Mentionable;

fn tick(ok: bool) -> &'static str {
    if ok {
        "✅"
    } else {
        "❌"
    }
}

/// Render the permission picture. Pure, so all three states are tested
/// without Discord.
fn render(p: &MusicPermissions) -> String {
    let mut problems: Vec<String> = Vec::new();

    if !p.text.is_whole() {
        problems.push(format!(
            "• **{}** in {} — now-playing posts will be skipped",
            p.text.missing(),
            p.text.channel.mention()
        ));
    }
    if let Some(v) = &p.voice {
        if !v.can_join() {
            problems.push(format!(
                "• **{}** in {} — /play will refuse to connect",
                v.missing(),
                v.channel.mention()
            ));
        }
    }

    // All-clear requires that we actually checked both halves. With no voice
    // channel to check we fall through to the table, which says so — claiming
    // "all clear" on a voice channel we never looked at would be a lie the
    // user only discovers when /play refuses.
    if problems.is_empty() {
        if let Some(v) = &p.voice {
            return format!(
                "✅ All clear — I have everything I need in {} and {}.",
                p.text.channel.mention(),
                v.channel.mention()
            );
        }
    }

    let mut out = String::from("🔍 **Permission check**\n");
    out.push_str(&format!(
        "**Text** {}  {} View  {} Send  {} Embed Links\n",
        p.text.channel.mention(),
        tick(p.text.view()),
        tick(p.text.send()),
        tick(p.text.embed()),
    ));
    match &p.voice {
        Some(v) => out.push_str(&format!(
            "**Voice** {}  {} Connect  {} Speak\n",
            v.channel.mention(),
            tick(v.connect()),
            tick(v.speak()),
        )),
        // Absent is not denied.
        None => out.push_str(
            "**Voice** — you're not in a voice channel, so I can't check \
             Connect/Speak. Join one and run this again.\n",
        ),
    }

    // Omitted entirely when nothing is wrong -- reached only via the
    // no-voice-channel path above, where there is nothing to report.
    if !problems.is_empty() {
        let n = problems.len();
        out.push_str(&format!(
            "\n**{n} problem{}**\n{}\n\nFix: Server Settings → Roles → CrackTunes",
            if n == 1 { "" } else { "s" },
            problems.join("\n"),
        ));
    }
    out
}

/// Report what the bot can and cannot do in this channel and your voice channel.
#[cfg(not(tarpaulin_include))]
#[poise::command(
    category = "Music",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral
)]
pub async fn diagnose(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let out = match resolve(ctx.cache(), guild_id, ctx.channel_id(), ctx.author().id) {
        Some(p) => render(&p),
        // Fail open in the wording too: say we could not read, not that
        // something is wrong.
        None => "I couldn't read my own permissions from cache just now — \
                 try again in a moment."
            .to_string(),
    };
    ctx.say(out).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::perms::{compute, TEXT_REQUIRED, VOICE_REQUIRED};
    use poise::serenity_prelude::all::{ChannelId, GenericChannelId, Permissions};

    fn text_ch() -> GenericChannelId {
        GenericChannelId::new(1)
    }
    fn voice_ch() -> ChannelId {
        ChannelId::new(2)
    }

    #[test]
    fn all_clear_says_so_and_lists_no_problems() {
        let p = compute(text_ch(), TEXT_REQUIRED, Some((voice_ch(), VOICE_REQUIRED)));
        let out = render(&p);
        assert!(out.contains("All clear"), "got {out}");
        assert!(
            !out.contains("problem"),
            "nothing is wrong, so say nothing: {out}"
        );
    }

    #[test]
    fn problems_are_counted_and_each_names_its_consequence() {
        let p = compute(
            text_ch(),
            TEXT_REQUIRED - Permissions::EMBED_LINKS,
            Some((voice_ch(), VOICE_REQUIRED - Permissions::SPEAK)),
        );
        let out = render(&p);
        assert!(out.contains("2 problems"), "got {out}");
        assert!(out.contains("Embed Links"), "got {out}");
        assert!(out.contains("Speak"), "got {out}");
        // A gap without its consequence is a riddle, not a diagnosis.
        assert!(out.contains("now-playing"), "got {out}");
        assert!(out.contains("refuse"), "got {out}");
    }

    #[test]
    fn no_voice_channel_asks_the_user_to_join_one_rather_than_reporting_a_denial() {
        let p = compute(text_ch(), TEXT_REQUIRED, None);
        let out = render(&p);
        // 🪤 Absent is not denied. Rendering this as a missing permission
        // would send someone to grant Connect when Connect is already
        // granted. Nor may this collapse into the all-clear line: we did not
        // check voice, so we must not claim it is fine.
        assert!(out.contains("not in a voice channel"), "got {out}");
        assert!(!out.contains("All clear"), "voice was never checked: {out}");
        assert!(!out.contains("\u{274c}"), "nothing is denied here: {out}");
        assert!(!out.contains("problem"), "nothing is wrong here: {out}");
    }

    #[test]
    fn one_problem_is_singular() {
        let p = compute(
            text_ch(),
            TEXT_REQUIRED - Permissions::EMBED_LINKS,
            Some((voice_ch(), VOICE_REQUIRED)),
        );
        let out = render(&p);
        assert!(out.contains("1 problem"), "got {out}");
        assert!(!out.contains("1 problems"), "got {out}");
    }
}
