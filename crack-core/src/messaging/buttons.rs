//! The now-playing buttons: their custom ids, the row on the status message,
//! and what a press does. Spec: the messaging-layer design, "PR 2".
//!
//! 🔑 An id names an intent, never a toggle, and carries everything a press
//! needs: buttons on an old status message still work after a restart, and a
//! Skip names the track it was drawn for, so a stale or doubled press cannot
//! skip the next song.
use crate::commands::permissions::music_access;
use crate::errors::CrackedError;
use crate::guild::operations::GuildSettingsOperations;
use crate::messaging::cards::Via;
use crate::messaging::courier;
use crate::messaging::message::CrackedMessage;
use crate::messaging::messages::{
    NP_BUTTON_PAUSE, NP_BUTTON_REPEAT, NP_BUTTON_RESUME, NP_BUTTON_SHUFFLE, NP_BUTTON_SKIP,
    OP_TRACK_STALE,
};
use crate::messaging::render::RenderCx;
use crate::messaging::transport::{DiscordPress, Press};
use crate::music::remote::{self, Control, ControlRefused, Echo};
use crate::Data;
use serenity::all::{
    ButtonStyle, ComponentInteraction, CreateActionRow, CreateButton, CreateComponent,
    GenericChannelId, GuildId, Member, ReactionType, UserId,
};
use serenity::small_fixed_array::FixedString;
use std::borrow::Cow;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Every now-playing button's custom id starts with this.
pub const NP_PREFIX: &str = "np:";

/// One accepted press per person per server per this long, across all the
/// buttons. A press inside it is acknowledged and dropped.
pub const NP_PRESS_WINDOW: Duration = Duration::from_secs(2);

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
    /// serenity ids are `NonMaxU64`, so `GuildId::new(u64::MAX)` panics; that and the
    /// never-issued 0 are rejected before it.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        let mut parts = id.strip_prefix(NP_PREFIX)?.split(':');
        let kind = parts.next()?;
        let guild = parts
            .next()?
            .parse::<u64>()
            .ok()
            .filter(|&n| n != 0 && n != u64::MAX)
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

/// A button that is only its symbol: no text label (#588).
fn button(b: NowPlayingButton, emoji: &'static str, style: ButtonStyle) -> CreateButton<'static> {
    CreateButton::new(b.custom_id())
        .emoji(ReactionType::Unicode(FixedString::from_static_trunc(emoji)))
        .style(style)
}

/// Who pressed, and where: the parts of the interaction `respond` reads.
pub(crate) struct Presser<'a> {
    /// `None` for a press outside a guild (a DM).
    pub guild: Option<GuildId>,
    pub user: UserId,
    pub is_bot: bool,
    pub member: Option<&'a Member>,
    pub channel: GenericChannelId,
}

/// A press on an `np:` button. Acknowledged first, before anything slow,
/// inside Discord's 3-second window; every later outcome the presser needs
/// to hear is a private follow-up. Success says nothing more: the echo line
/// (if the guild has echoes on) and the re-rendered status are the answer.
/// A second press by the same person in the same server inside
/// [`NP_PRESS_WINDOW`] is acknowledged and dropped. A server that turned the
/// buttons off (`/buttons`) refuses every press privately.
pub async fn handle(data: &Data, ctx: &serenity::all::Context, interaction: &ComponentInteraction) {
    let press = DiscordPress {
        http: &ctx.http,
        interaction,
    };
    let who = Presser {
        guild: interaction.guild_id,
        user: interaction.user.id,
        is_bot: interaction.user.bot(),
        member: interaction.member.as_deref(),
        channel: interaction.channel_id,
    };
    let data_arc = Arc::new(data.clone());
    respond(
        data,
        &press,
        &interaction.data.custom_id,
        who,
        Instant::now(),
        |guild, user, via, c| {
            remote::control(
                data_arc,
                ctx.http.clone(),
                ctx.cache.clone(),
                guild,
                user,
                via,
                c,
            )
        },
    )
    .await;
}

/// [`handle`] without Discord: `run` is the control.
pub(crate) async fn respond<F, Fut>(
    data: &Data,
    press: &dyn Press,
    custom_id: &str,
    who: Presser<'_>,
    now: Instant,
    run: F,
) where
    F: FnOnce(GuildId, UserId, Via, Control) -> Fut,
    Fut: Future<Output = Result<Option<Echo>, ControlRefused>>,
{
    courier::acknowledge(press).await;
    let cx = RenderCx::now();
    let button = match NowPlayingButton::parse(custom_id) {
        Some(b) if who.guild == Some(b.guild()) => b,
        _ => {
            tracing::warn!(
                "np: press with an unusable id {custom_id:?} from {}",
                who.user
            );
            courier::answer_privately(press, &CrackedMessage::ButtonOutOfDate, &cx).await;
            return;
        },
    };
    let guild = button.guild();
    // One accepted press per person per server per window; the rest are
    // acknowledged above and dropped here, before anything can answer them.
    if !data.np_presses.allow((guild, who.user), now) {
        return;
    }
    // A server can turn the buttons off (`/buttons`); old messages keep theirs.
    if !data.get_now_playing_buttons(guild).await {
        courier::answer_privately(press, &CrackedMessage::NowPlayingButtonsDisabled, &cx).await;
        return;
    }
    if let Err(err) = music_access(
        data,
        guild,
        who.member,
        who.is_bot,
        who.channel,
        button.command(),
    )
    .await
    {
        courier::answer_privately(press, &CrackedMessage::CrackedError(err), &cx).await;
        return;
    }
    if let Err(refused) = run(guild, who.user, Via::Button, button.control()).await {
        courier::answer_privately(press, &refusal(refused), &cx).await;
    }
}

/// A refused control, in the words the slash commands use.
fn refusal(r: ControlRefused) -> CrackedMessage {
    match r {
        ControlRefused::NotPlaying => CrackedMessage::CrackedError(CrackedError::NothingPlaying),
        ControlRefused::GameInProgress => {
            CrackedMessage::CrackedError(CrackedError::GameInProgress)
        },
        // Only a Skip drawn for an earlier track gets here from a button.
        ControlRefused::Conflict => {
            CrackedMessage::CrackedError(CrackedError::Other(OP_TRACK_STALE))
        },
        ControlRefused::Failed => CrackedMessage::Error,
    }
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

    /// Plan 2026-10-07-now-playing-buttons, Review Focus 3: nothing malformed parses, and nothing panics
    /// (`GuildId::new(0)` would).
    #[test]
    fn malformed_ids_are_rejected() {
        for bad in [
            "",
            "np:",
            "np:pause",
            "np:pause:",
            "np:pause:0",
            "np:pause:18446744073709551615",
            "np:skip:18446744073709551615:67e55044-10b1-426f-9247-bb680e5fe0c8",
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
    /// Each button's `(custom_id, emoji, style)`. A button has no text label
    /// (#588): a labelled row wrapped unevenly on narrow screens.
    fn ids_emoji_styles(v: &serde_json::Value) -> Vec<(String, String, u64)> {
        v["components"]
            .as_array()
            .expect("an action row")
            .iter()
            .map(|b| {
                assert!(b.get("label").is_none(), "a button with a label: {b}");
                (
                    b["custom_id"].as_str().unwrap().to_owned(),
                    b["emoji"]["name"].as_str().unwrap().to_owned(),
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
            ids_emoji_styles(&row_json(&c)),
            vec![
                ("np:pause:123456789012345678".into(), "⏸️".into(), 2),
                (
                    "np:skip:123456789012345678:67e55044-10b1-426f-9247-bb680e5fe0c8".into(),
                    "⏭️".into(),
                    2
                ),
                ("np:repeat-on:123456789012345678".into(), "🔁".into(), 2),
                ("np:shuffle:123456789012345678".into(), "🔀".into(), 2),
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
        let got = ids_emoji_styles(&row_json(&c));
        assert_eq!(
            got[0],
            ("np:resume:123456789012345678".into(), "▶️".into(), 2)
        );
        assert_eq!(
            got[2],
            ("np:repeat-off:123456789012345678".into(), "🔁".into(), 3)
        );
    }

    use crate::messaging::test_support::{FakePress, PressOp};
    use crate::music::remote::{ControlRefused, Echo};
    use crate::Data;
    use serenity::all::{GenericChannelId, UserId};
    use std::sync::Mutex;
    use std::time::Instant;

    const U: UserId = UserId::new(42);
    const CH: GenericChannelId = GenericChannelId::new(10);

    fn presser(guild: Option<GuildId>) -> Presser<'static> {
        Presser {
            guild,
            user: U,
            is_bot: false,
            member: None,
            channel: CH,
        }
    }

    /// Records what `respond` asked to run; answers with `answer`; presses at `now`.
    async fn press_at(
        data: &Data,
        id: &str,
        who: Presser<'_>,
        now: Instant,
        answer: Result<Option<Echo>, ControlRefused>,
    ) -> (Vec<PressOp>, Vec<(GuildId, UserId, Via, Control)>) {
        let p = FakePress::default();
        let ran = Mutex::new(Vec::new());
        respond(data, &p, id, who, now, |g, u, v, c| {
            ran.lock().unwrap().push((g, u, v, c));
            async move { answer }
        })
        .await;
        (p.ops(), ran.into_inner().unwrap())
    }

    /// [`press_at`] now.
    async fn press_with(
        data: &Data,
        id: &str,
        who: Presser<'_>,
        answer: Result<Option<Echo>, ControlRefused>,
    ) -> (Vec<PressOp>, Vec<(GuildId, UserId, Via, Control)>) {
        press_at(data, id, who, Instant::now(), answer).await
    }

    fn private(text: &str) -> PressOp {
        PressOp::Followup {
            ephemeral: true,
            text: text.into(),
        }
    }

    #[tokio::test]
    async fn a_press_is_acknowledged_first_then_runs_its_control_and_says_nothing_more() {
        let data = Data::default();
        let id = NowPlayingButton::Skip {
            guild: G,
            track: uuid(),
        }
        .custom_id();
        let (ops, ran) = press_with(
            &data,
            &id,
            presser(Some(G)),
            Ok(Some(Echo::Skipped { title: None })),
        )
        .await;
        assert_eq!(ops, vec![PressOp::Acknowledge]);
        assert_eq!(
            ran,
            vec![(G, U, Via::Button, Control::Skip { expect: uuid() })]
        );
    }

    /// A control that changed nothing still says nothing to the presser: the
    /// status message is the answer.
    #[tokio::test]
    async fn a_no_op_press_is_silent() {
        let data = Data::default();
        let id = NowPlayingButton::Pause { guild: G }.custom_id();
        let (ops, ran) = press_with(&data, &id, presser(Some(G)), Ok(None)).await;
        assert_eq!(ops, vec![PressOp::Acknowledge]);
        assert_eq!(ran.len(), 1);
    }

    /// A forged id never runs a control (Plan 2026-10-07-now-playing-buttons, Review Focus 3).
    #[tokio::test]
    async fn a_malformed_id_is_out_of_date_and_runs_nothing() {
        let data = Data::default();
        let (ops, ran) =
            press_with(&data, "np:skip:1:not-a-uuid", presser(Some(G)), Ok(None)).await;
        assert_eq!(
            ops,
            vec![PressOp::Acknowledge, private("This button is out of date.")]
        );
        assert!(ran.is_empty());
    }

    /// An id for another guild than the one the press came from (a forged id).
    #[tokio::test]
    async fn an_id_for_another_guild_is_out_of_date_and_runs_nothing() {
        let data = Data::default();
        let id = NowPlayingButton::Pause {
            guild: GuildId::new(999),
        }
        .custom_id();
        let (ops, ran) = press_with(&data, &id, presser(Some(G)), Ok(None)).await;
        assert_eq!(
            ops,
            vec![PressOp::Acknowledge, private("This button is out of date.")]
        );
        assert!(ran.is_empty());
        let (_, ran) = press_with(&data, &id, presser(None), Ok(None)).await;
        assert!(ran.is_empty(), "a press from a DM runs nothing");
    }

    /// Plan 2026-10-07-now-playing-buttons, Review Focus 4: refused in the slash command's words, privately.
    #[tokio::test]
    async fn a_press_outside_the_music_channel_is_refused_privately() {
        let data = Data::default();
        let mut s = crate::guild::settings::GuildSettings::new(G, None, None);
        s.set_music_channel(77);
        data.guild_settings_map.write().await.insert(G, s);
        let id = NowPlayingButton::Shuffle { guild: G }.custom_id();
        let (ops, ran) = press_with(&data, &id, presser(Some(G)), Ok(None)).await;
        assert_eq!(
            ops,
            vec![
                PressOp::Acknowledge,
                private("⚠️ You are not in the music channel! Use <#77>"),
            ]
        );
        assert!(ran.is_empty());
    }

    /// Plan 2026-10-07-now-playing-buttons, Review Focus 1 and 2: a stale skip, and nothing playing.
    #[tokio::test]
    async fn refusals_from_the_control_are_answered_privately() {
        let id = NowPlayingButton::Skip {
            guild: G,
            track: uuid(),
        }
        .custom_id();
        for (refused, words) in [
            (ControlRefused::Conflict, "That track is no longer playing"),
            (ControlRefused::NotPlaying, "🔈 Nothing is playing!"),
            (
                ControlRefused::GameInProgress,
                "🎭 A game is running; that command would break the rounds. `/gp skip`, `/gp close` or `/gp end` instead.",
            ),
            (ControlRefused::Failed, "Fatality! Something went wrong ☹️"),
        ] {
            let data = Data::default();
            let (ops, _) = press_with(&data, &id, presser(Some(G)), Err(refused)).await;
            assert_eq!(ops, vec![PressOp::Acknowledge, private(words)], "{refused:?}");
        }
    }

    fn pause() -> String {
        NowPlayingButton::Pause { guild: G }.custom_id()
    }

    async fn buttons_off(data: &Data) {
        let mut s = crate::guild::settings::GuildSettings::new(G, None, None);
        s.now_playing_buttons = false;
        data.guild_settings_map.write().await.insert(G, s);
    }

    #[tokio::test]
    async fn a_second_press_inside_the_window_is_acknowledged_and_nothing_else() {
        let data = Data::default();
        let t0 = Instant::now();
        let (_, ran) = press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
        let (ops, ran) = press_at(
            &data,
            &pause(),
            presser(Some(G)),
            t0 + NP_PRESS_WINDOW - Duration::from_millis(1),
            Ok(None),
        )
        .await;
        assert_eq!(ops, vec![PressOp::Acknowledge]);
        assert!(ran.is_empty());
        let (_, ran) = press_at(
            &data,
            &pause(),
            presser(Some(G)),
            t0 + NP_PRESS_WINDOW,
            Ok(None),
        )
        .await;
        assert_eq!(ran.len(), 1, "after the window it runs again");
    }

    /// One window across all the buttons: a Skip right after a Pause is dropped too.
    #[tokio::test]
    async fn the_window_covers_every_button() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        let shuffle = NowPlayingButton::Shuffle { guild: G }.custom_id();
        let (_, ran) = press_at(&data, &shuffle, presser(Some(G)), t0, Ok(None)).await;
        assert!(ran.is_empty());
    }

    #[tokio::test]
    async fn another_user_is_not_held_up_by_the_window() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        let other = Presser {
            user: UserId::new(43),
            ..presser(Some(G))
        };
        let (_, ran) = press_at(&data, &pause(), other, t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
    }

    /// The window is per server too: the same person pressing in another
    /// server is not held up.
    #[tokio::test]
    async fn another_server_is_not_held_up_by_the_window() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        let g2 = GuildId::new(222);
        let id = NowPlayingButton::Pause { guild: g2 }.custom_id();
        let (_, ran) = press_at(&data, &id, presser(Some(g2)), t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
    }

    /// Review Focus 2: a press that never parses for this guild is not a press
    /// here, and must not use up the presser's window.
    #[tokio::test]
    async fn an_out_of_date_press_does_not_use_up_the_window() {
        let data = Data::default();
        let t0 = Instant::now();
        press_at(
            &data,
            "np:skip:1:not-a-uuid",
            presser(Some(G)),
            t0,
            Ok(None),
        )
        .await;
        let (_, ran) = press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        assert_eq!(ran.len(), 1);
    }

    #[tokio::test]
    async fn with_the_buttons_off_a_press_is_refused_privately_and_runs_nothing() {
        let data = Data::default();
        buttons_off(&data).await;
        let (ops, ran) = press_with(&data, &pause(), presser(Some(G)), Ok(None)).await;
        assert_eq!(
            ops,
            vec![
                PressOp::Acknowledge,
                private("The now-playing buttons are turned off in this server.")
            ]
        );
        assert!(ran.is_empty());
    }

    /// Review Focus 3: mashing an old button after `/buttons` off draws one
    /// refusal per window, not one per press.
    #[tokio::test]
    async fn mashing_with_the_buttons_off_is_refused_once_per_window() {
        let data = Data::default();
        buttons_off(&data).await;
        let t0 = Instant::now();
        let (first, _) = press_at(&data, &pause(), presser(Some(G)), t0, Ok(None)).await;
        assert_eq!(first.len(), 2, "acknowledged and refused");
        for ms in [1, 500, 1999] {
            let (ops, _) = press_at(
                &data,
                &pause(),
                presser(Some(G)),
                t0 + Duration::from_millis(ms),
                Ok(None),
            )
            .await;
            assert_eq!(ops, vec![PressOp::Acknowledge], "at +{ms}ms");
        }
    }
}
