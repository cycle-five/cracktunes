//! The now-playing buttons: their custom ids, the row on the status message,
//! and (Task 7) what a press does. Spec: the messaging-layer design, "PR 2".
//!
//! 🔑 An id names an intent, never a toggle, and carries everything a press
//! needs: buttons on an old status message still work after a restart, and a
//! Skip names the track it was drawn for, so a stale or doubled press cannot
//! skip the next song.
use crate::messaging::messages::{
    NP_BUTTON_PAUSE, NP_BUTTON_REPEAT, NP_BUTTON_RESUME, NP_BUTTON_SHUFFLE, NP_BUTTON_SKIP,
};
use crate::music::remote::Control;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateComponent, GuildId};
use std::borrow::Cow;
use uuid::Uuid;

/// Every now-playing button's custom id starts with this.
pub const NP_PREFIX: &str = "np:";

/// One now-playing button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NowPlayingButton {
    Pause {
        guild: GuildId,
    },
    Resume {
        guild: GuildId,
    },
    /// Only if `track` is still the one playing.
    Skip {
        guild: GuildId,
        track: Uuid,
    },
    RepeatOn {
        guild: GuildId,
    },
    RepeatOff {
        guild: GuildId,
    },
    Shuffle {
        guild: GuildId,
    },
}

impl NowPlayingButton {
    #[must_use]
    pub fn custom_id(&self) -> String {
        match *self {
            Self::Pause { guild } => format!("{NP_PREFIX}pause:{guild}"),
            Self::Resume { guild } => format!("{NP_PREFIX}resume:{guild}"),
            Self::Skip { guild, track } => format!("{NP_PREFIX}skip:{guild}:{track}"),
            Self::RepeatOn { guild } => format!("{NP_PREFIX}repeat-on:{guild}"),
            Self::RepeatOff { guild } => format!("{NP_PREFIX}repeat-off:{guild}"),
            Self::Shuffle { guild } => format!("{NP_PREFIX}shuffle:{guild}"),
        }
    }

    /// The button an id names, or `None` for anything malformed. Never panics:
    /// a zero guild id is rejected before `GuildId::new`, which panics on it.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        let mut parts = id.strip_prefix(NP_PREFIX)?.split(':');
        let kind = parts.next()?;
        let guild = parts
            .next()?
            .parse::<u64>()
            .ok()
            .filter(|&n| n != 0)
            .map(GuildId::new)?;
        let button = match kind {
            "pause" => Self::Pause { guild },
            "resume" => Self::Resume { guild },
            "skip" => Self::Skip {
                guild,
                track: Uuid::parse_str(parts.next()?).ok()?,
            },
            "repeat-on" => Self::RepeatOn { guild },
            "repeat-off" => Self::RepeatOff { guild },
            "shuffle" => Self::Shuffle { guild },
            _ => return None,
        };
        // Nothing may follow what the kind needs.
        parts.next().is_none().then_some(button)
    }

    #[must_use]
    pub fn guild(&self) -> GuildId {
        match *self {
            Self::Pause { guild }
            | Self::Resume { guild }
            | Self::Skip { guild, .. }
            | Self::RepeatOn { guild }
            | Self::RepeatOff { guild }
            | Self::Shuffle { guild } => guild,
        }
    }

    #[must_use]
    pub fn control(&self) -> Control {
        match *self {
            Self::Pause { .. } => Control::Pause,
            Self::Resume { .. } => Control::Resume,
            Self::Skip { track, .. } => Control::Skip { expect: track },
            Self::RepeatOn { .. } => Control::Repeat { on: true },
            Self::RepeatOff { .. } => Control::Repeat { on: false },
            Self::Shuffle { .. } => Control::Shuffle,
        }
    }

    /// The slash command this button stands in for: the name `/gp` blocks
    /// while a game runs, and the op the audit log records.
    #[must_use]
    pub fn command(&self) -> &'static str {
        match self {
            Self::Pause { .. } => "pause",
            Self::Resume { .. } => "resume",
            Self::Skip { .. } => "skip",
            Self::RepeatOn { .. } | Self::RepeatOff { .. } => "repeat",
            Self::Shuffle { .. } => "shuffle",
        }
    }
}

/// What the status message's buttons are drawn from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Controls {
    pub guild: GuildId,
    /// The playing track, for Skip.
    pub track: Uuid,
    pub paused: bool,
    pub looping: bool,
}

/// The one action row: Pause or Resume, Skip, Repeat (green while on), Shuffle.
pub fn now_playing_row(c: &Controls) -> CreateComponent<'static> {
    let guild = c.guild;
    let play = if c.paused {
        button(
            NowPlayingButton::Resume { guild },
            NP_BUTTON_RESUME,
            ButtonStyle::Secondary,
        )
    } else {
        button(
            NowPlayingButton::Pause { guild },
            NP_BUTTON_PAUSE,
            ButtonStyle::Secondary,
        )
    };
    let repeat = if c.looping {
        button(
            NowPlayingButton::RepeatOff { guild },
            NP_BUTTON_REPEAT,
            ButtonStyle::Success,
        )
    } else {
        button(
            NowPlayingButton::RepeatOn { guild },
            NP_BUTTON_REPEAT,
            ButtonStyle::Secondary,
        )
    };
    CreateComponent::ActionRow(CreateActionRow::Buttons(Cow::Owned(vec![
        play,
        button(
            NowPlayingButton::Skip {
                guild,
                track: c.track,
            },
            NP_BUTTON_SKIP,
            ButtonStyle::Secondary,
        ),
        repeat,
        button(
            NowPlayingButton::Shuffle { guild },
            NP_BUTTON_SHUFFLE,
            ButtonStyle::Secondary,
        ),
    ])))
}

fn button(b: NowPlayingButton, label: &'static str, style: ButtonStyle) -> CreateButton<'static> {
    CreateButton::new(b.custom_id()).label(label).style(style)
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: GuildId = GuildId::new(123456789012345678);
    fn uuid() -> Uuid {
        Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap()
    }
    fn every() -> Vec<NowPlayingButton> {
        vec![
            NowPlayingButton::Pause { guild: G },
            NowPlayingButton::Resume { guild: G },
            NowPlayingButton::Skip {
                guild: G,
                track: uuid(),
            },
            NowPlayingButton::RepeatOn { guild: G },
            NowPlayingButton::RepeatOff { guild: G },
            NowPlayingButton::Shuffle { guild: G },
        ]
    }

    #[test]
    fn ids_read_as_the_spec_spells_them() {
        let ids: Vec<String> = every().iter().map(NowPlayingButton::custom_id).collect();
        assert_eq!(
            ids,
            vec![
                "np:pause:123456789012345678",
                "np:resume:123456789012345678",
                "np:skip:123456789012345678:67e55044-10b1-426f-9247-bb680e5fe0c8",
                "np:repeat-on:123456789012345678",
                "np:repeat-off:123456789012345678",
                "np:shuffle:123456789012345678",
            ]
        );
    }

    #[test]
    fn every_id_round_trips() {
        for b in every() {
            assert_eq!(NowPlayingButton::parse(&b.custom_id()), Some(b), "{b:?}");
        }
    }

    /// Discord's custom_id limit is 100 characters; the longest is 65.
    #[test]
    fn the_longest_id_fits_discords_limit() {
        let b = NowPlayingButton::Skip {
            guild: GuildId::new(u64::MAX - 1),
            track: uuid(),
        };
        assert_eq!(b.custom_id().len(), 65);
    }

    /// Review Focus 3: nothing malformed parses, and nothing panics
    /// (`GuildId::new(0)` would).
    #[test]
    fn malformed_ids_are_rejected() {
        for bad in [
            "",
            "np:",
            "np:pause",
            "np:pause:",
            "np:pause:0",
            "np:pause:-1",
            "np:pause:abc",
            "np:pause:1:extra",
            "np:skip:1",
            "np:skip:1:not-a-uuid",
            "np:skip:1:67e55044-10b1-426f-9247-bb680e5fe0c8:x",
            "np:dance:1",
            "np:repeat:1",
            "gp:pause:1",
            "pause:1",
        ] {
            assert_eq!(NowPlayingButton::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn each_button_runs_its_control_and_names_its_command() {
        use crate::music::remote::Control;
        let got: Vec<(Control, &str)> =
            every().iter().map(|b| (b.control(), b.command())).collect();
        assert_eq!(
            got,
            vec![
                (Control::Pause, "pause"),
                (Control::Resume, "resume"),
                (Control::Skip { expect: uuid() }, "skip"),
                (Control::Repeat { on: true }, "repeat"),
                (Control::Repeat { on: false }, "repeat"),
                (Control::Shuffle, "shuffle"),
            ]
        );
        assert!(every().iter().all(|b| b.guild() == G));
    }

    fn row_json(c: &Controls) -> serde_json::Value {
        serde_json::to_value(now_playing_row(c)).unwrap()
    }
    fn ids_labels_styles(v: &serde_json::Value) -> Vec<(String, String, u64)> {
        v["components"]
            .as_array()
            .expect("an action row")
            .iter()
            .map(|b| {
                (
                    b["custom_id"].as_str().unwrap().to_owned(),
                    b["label"].as_str().unwrap().to_owned(),
                    b["style"].as_u64().unwrap(),
                )
            })
            .collect()
    }

    /// Playing, repeat off: Pause, Skip, Repeat (grey, turns it on), Shuffle.
    #[test]
    fn a_playing_track_shows_pause_and_repeat_off() {
        let c = Controls {
            guild: G,
            track: uuid(),
            paused: false,
            looping: false,
        };
        assert_eq!(
            ids_labels_styles(&row_json(&c)),
            vec![
                ("np:pause:123456789012345678".into(), "⏸ Pause".into(), 2),
                (
                    "np:skip:123456789012345678:67e55044-10b1-426f-9247-bb680e5fe0c8".into(),
                    "⏭ Skip".into(),
                    2
                ),
                (
                    "np:repeat-on:123456789012345678".into(),
                    "🔁 Repeat".into(),
                    2
                ),
                (
                    "np:shuffle:123456789012345678".into(),
                    "🔀 Shuffle".into(),
                    2
                ),
            ]
        );
    }

    /// Paused and on repeat: Resume, and Repeat green (style 3), which turns it off.
    #[test]
    fn a_paused_track_on_repeat_shows_resume_and_repeat_on() {
        let c = Controls {
            guild: G,
            track: uuid(),
            paused: true,
            looping: true,
        };
        let got = ids_labels_styles(&row_json(&c));
        assert_eq!(
            got[0],
            ("np:resume:123456789012345678".into(), "▶ Resume".into(), 2)
        );
        assert_eq!(
            got[2],
            (
                "np:repeat-off:123456789012345678".into(),
                "🔁 Repeat".into(),
                3
            )
        );
    }
}
