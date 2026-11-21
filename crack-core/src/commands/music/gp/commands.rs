use super::playback::{gp_after_close, gp_open_round, gp_voice_players, GpPlayback};
use crate::music::from_ctx::ActorFromCtx;
use crate::{
    commands::cmd_check_music,
    commands::get_call_or_join_author,
    commands::music::gp_prompts::{
        draw_prompts, draw_wrong_prompts, GpCategories, GpCategory, WrongPromptTheme,
        WrongPromptThemes,
    },
    errors::CrackedError,
    http_utils::SendMessageParams,
    messaging::message::CrackedMessage,
    messaging::messages::{
        GP_PICK_CANCELLED, GP_PICK_CHOSEN, GP_PICK_NOT_HOST, GP_PICK_TEXT, GP_PICK_TIMED_OUT,
        GP_SCOREBOARD, SPOTIFY_GP_ONE_SONG, SPOTIFY_NOTHING_PLAYABLE,
    },
    messaging::{
        courier,
        render::RenderCx,
        transport::{DiscordPress, DiscordTransport, Press},
    },
    music::queue::{force_skip_top_track, stop_queue},
    music::PlaybackOwner,
    poise_ext::PoiseContextExt,
    sources::sleevenote,
    Context, CrackedResult, Error,
};
use ::serenity::all::{
    AutocompleteChoice, ChannelId, ComponentInteractionDataKind, CreateAutocompleteResponse,
    GuildId, ResolvedValue, UserId,
};
use crack_gp::gp::{
    gp_picked, gp_rendered, now, user_refs, GpCard, GpClip, GpPhase, GpReveal, GpVoteFullOutcome,
    GpVoteSkipOutcome, GP_DEFAULT_CLIPS, GP_DEFAULT_CLIP_LENGTH_SECS, GP_DEFAULT_CLIP_START_SECS,
    GP_DEFAULT_ROUNDS, GP_DEFAULT_TIMER_SECS, GP_MAX_CLIP_LENGTH_SECS, GP_MAX_CLIP_START_SECS,
    GP_MAX_PLAYERS, GP_MAX_ROUNDS, GP_MAX_TIMER_SECS, GP_MIN_CLIP_LENGTH_SECS, GP_MIN_TIMER_SECS,
    GP_PARK_GRACE_SECS, GP_PICK_CANCEL_ID, GP_PICK_START_ID, GP_PICK_TIMEOUT_SECS,
    GP_WRONG_PROMPT_MIN_PLAYERS,
};
use crack_types::QueryType;
use poise::{serenity_prelude::CollectComponentInteractions, ChoiceParameter};
use songbird::Call;
use std::{borrow::Cow, str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;

// ------------------------------------------------------------------
// Commands
// ------------------------------------------------------------------

async fn author_display_name(ctx: Context<'_>) -> String {
    match ctx.author_member().await {
        Some(m) => m.display_name().to_string(),
        None => ctx.author().name.to_string(),
    }
}

fn gp_vc_members(ctx: Context<'_>, vc: ChannelId) -> Vec<UserId> {
    ctx.guild_id().map_or_else(Vec::new, |guild_id| {
        gp_voice_players(&ctx.serenity_context().cache, guild_id, vc)
    })
}

/// The game is for the people in the room: acting on a running game means being in
/// its voice channel. `/gp submit` needs only this much -- submitting is how you
/// join -- while everything else also goes through [`gp_require_player`].
fn gp_require_in_game_vc(ctx: Context<'_>, guild_id: GuildId) -> CrackedResult<ChannelId> {
    let game_vc = ctx
        .data()
        .gp_voice_channel(guild_id)
        .ok_or(CrackedError::NoGameInProgress)?;
    if ctx.author_vc() != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel);
    }
    Ok(game_vc)
}

/// In the voice channel *and* actually playing. `/gp end` is the deliberate
/// exception to both, so an admin can always kill a game from outside it.
fn gp_require_player(ctx: Context<'_>, guild_id: GuildId) -> CrackedResult<ChannelId> {
    let game_vc = gp_require_in_game_vc(ctx, guild_id)?;
    ctx.data().gp_require_player(guild_id, ctx.author().id)?;
    Ok(game_vc)
}

fn gp_playback(ctx: Context<'_>, call: Arc<Mutex<Call>>, guild_id: GuildId) -> GpPlayback {
    GpPlayback {
        data: ctx.data().clone(),
        transport: Arc::new(DiscordTransport::of(ctx.serenity_context())),
        cache: Some(ctx.serenity_context().cache.clone()),
        call,
        guild_id,
    }
}

/// "What's your song?" party game: a prompt, secret submissions, guess whose is whose.
#[poise::command(
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    aliases("guiltypleasure"),
    subcommands(
        "gp_start",
        "gp_submit",
        "gp_close",
        "gp_skip",
        "gp_voteskip",
        "gp_votefull",
        "gp_status",
        "gp_end"
    )
)]
pub async fn gp(ctx: Context<'_>) -> Result<(), Error> {
    courier::reply_as(ctx, GpCard::Rules.into(), false).await?;
    Ok(())
}

enum GpPromptPool {
    Classic(GpCategories),
    WrongPrompt(WrongPromptThemes),
}

impl GpPromptPool {
    fn display(&self) -> String {
        match self {
            Self::Classic(categories) => categories.display(),
            Self::WrongPrompt(themes) => format!("Wrong Prompt · {}", themes.display()),
        }
    }
}

enum GpPicker {
    Classic(Vec<GpCategory>),
    WrongPrompt(Vec<WrongPromptTheme>),
}

impl GpPicker {
    fn instructions(&self) -> &'static str {
        match self {
            Self::Classic(_) => GP_PICK_TEXT,
            Self::WrongPrompt(_) => "Tick the Wrong Prompt themes to play, then press **Start**.",
        }
    }

    fn card(&self, text: &str, open: bool) -> GpCard {
        match self {
            Self::Classic(picked) => GpCard::Picker {
                text: text.into(),
                picked: picked.clone(),
                open,
            },
            Self::WrongPrompt(picked) => GpCard::WrongPromptPicker {
                text: text.into(),
                picked: picked.clone(),
                open,
            },
        }
    }

    fn update<'a>(&mut self, values: impl IntoIterator<Item = &'a str>) {
        match self {
            Self::Classic(picked) => *picked = gp_picked(values),
            Self::WrongPrompt(picked) => {
                *picked = values
                    .into_iter()
                    .filter_map(WrongPromptTheme::from_key)
                    .collect();
            },
        }
    }

    fn selection(&self) -> Option<GpPromptPool> {
        match self {
            Self::Classic(picked) => {
                GpCategories::new(picked.iter().copied()).map(GpPromptPool::Classic)
            },
            Self::WrongPrompt(picked) => {
                WrongPromptThemes::new(picked.iter().copied()).map(GpPromptPool::WrongPrompt)
            },
        }
    }
}

/// Collect the host's multi-select before the game exists. A cancellation or
/// timeout retires the picker without starting anything.
async fn gp_pick_prompts(
    ctx: Context<'_>,
    mut picker: GpPicker,
) -> Result<Option<GpPromptPool>, Error> {
    let host = ctx.author().id;
    let instructions = picker.instructions();
    let reply = courier::reply_as(ctx, picker.card(instructions, true).into(), true).await?;
    let message_id = reply.message().await?.id;
    loop {
        // A collector per click, so the timeout is how long the host has sat
        // idle rather than how long the whole pick has taken.
        let Some(mci) = message_id
            .collect_component_interactions(ctx.serenity_context())
            .timeout(Duration::from_secs(GP_PICK_TIMEOUT_SECS))
            .next()
            .await
        else {
            courier::retire_reply(ctx, &reply, picker.card(GP_PICK_TIMED_OUT, false).into())
                .await?;
            return Ok(None);
        };
        let press = DiscordPress {
            http: ctx.http(),
            interaction: &mci,
        };
        // Only a prefix command's picker is public; from a slash command nobody
        // else can see it.
        if mci.user.id != host {
            courier::respond(
                &press,
                &GpCard::Line(GP_PICK_NOT_HOST.into()).into(),
                &RenderCx::now(),
                true,
            )
            .await?;
            continue;
        }
        let id = mci.data.custom_id.as_str();
        if id == GP_PICK_CANCEL_ID {
            gp_pick_update(&press, picker.card(GP_PICK_CANCELLED, false)).await?;
            return Ok(None);
        }
        if id == GP_PICK_START_ID {
            if let Some(selection) = picker.selection() {
                let chosen = format!("{GP_PICK_CHOSEN} {}", selection.display());
                gp_pick_update(&press, picker.card(&chosen, false)).await?;
                return Ok(Some(selection));
            }
        } else if let ComponentInteractionDataKind::StringSelect { values } = &mci.data.kind {
            picker.update(values.iter().map(String::as_str));
        }
        gp_pick_update(&press, picker.card(instructions, true)).await?;
    }
}

/// Answer a picker click by redrawing the picker.
async fn gp_pick_update(press: &dyn Press, card: GpCard) -> Result<(), Error> {
    courier::update(press, &card.into(), &RenderCx::now()).await?;
    Ok(())
}

/// Classic GP category names and the aliases prefix commands accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum GpCategoryChoice {
    /// First, so it tops the dropdown; also what `/gp start` plays when no
    /// category is given. `mixed` is the name it had before.
    #[name = "🎲 Random"]
    #[name = "random"]
    #[name = "mixed"]
    Random,
    #[name = "☑️ Pick several…"]
    #[name = "pick"]
    PickSeveral,
    #[name = "🥹 Nostalgia"]
    #[name = "nostalgia"]
    Nostalgia,
    #[name = "🔥 Slightly More Dangerous"]
    #[name = "dangerous"]
    Dangerous,
    #[name = "🎶 The Really Good Game Prompts"]
    #[name = "good_game"]
    GoodGame,
    #[name = "🚗 Car / Driving"]
    #[name = "car"]
    Car,
    #[name = "🌿 Altered-State / Chill"]
    #[name = "chill"]
    Chill,
    #[name = "😭 Emotional"]
    #[name = "emotional"]
    Emotional,
    #[name = "🤢 Bad Music"]
    #[name = "bad_music"]
    BadMusic,
    #[name = "🎧 Hyper-Specific"]
    #[name = "hyper_specific"]
    HyperSpecific,
    #[name = "🖤 Weirdly Revealing"]
    #[name = "revealing"]
    Revealing,
    #[name = "😂 Game Chaos"]
    #[name = "chaos"]
    Chaos,
    #[name = "⚡ One-Worders"]
    #[name = "one_worders"]
    OneWorders,
    #[name = "🎤 Social / Go-To"]
    #[name = "social"]
    Social,
    #[name = "😈 Guilty Pleasures / Secret Taste"]
    #[name = "guilty"]
    Guilty,
    #[name = "💋 Sex / Romance / Attraction"]
    #[name = "romance"]
    Romance,
    #[name = "🥀 Emotional Damage"]
    #[name = "damage"]
    Damage,
    #[name = "🕺 Chaotic / Funny"]
    #[name = "funny"]
    Funny,
    #[name = "🧠 Personality Reveals"]
    #[name = "personality"]
    Personality,
}

impl From<GpCategoryChoice> for GpCategory {
    fn from(choice: GpCategoryChoice) -> Self {
        match choice {
            GpCategoryChoice::Random => Self::Random,
            GpCategoryChoice::PickSeveral => Self::PickSeveral,
            GpCategoryChoice::Nostalgia => Self::Nostalgia,
            GpCategoryChoice::Dangerous => Self::Dangerous,
            GpCategoryChoice::GoodGame => Self::GoodGame,
            GpCategoryChoice::Car => Self::Car,
            GpCategoryChoice::Chill => Self::Chill,
            GpCategoryChoice::Emotional => Self::Emotional,
            GpCategoryChoice::BadMusic => Self::BadMusic,
            GpCategoryChoice::HyperSpecific => Self::HyperSpecific,
            GpCategoryChoice::Revealing => Self::Revealing,
            GpCategoryChoice::Chaos => Self::Chaos,
            GpCategoryChoice::OneWorders => Self::OneWorders,
            GpCategoryChoice::Social => Self::Social,
            GpCategoryChoice::Guilty => Self::Guilty,
            GpCategoryChoice::Romance => Self::Romance,
            GpCategoryChoice::Damage => Self::Damage,
            GpCategoryChoice::Funny => Self::Funny,
            GpCategoryChoice::Personality => Self::Personality,
        }
    }
}

/// `/gp start`'s reveal dropdown. The first `#[name]` is what Discord shows;
/// the second is what a prefix command types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum GpRevealChoice {
    #[name = "🤐 At the end of the round"]
    #[name = "round"]
    Round,
    #[name = "🎉 After each song"]
    #[name = "song"]
    Song,
}

impl From<GpRevealChoice> for GpReveal {
    fn from(choice: GpRevealChoice) -> Self {
        match choice {
            GpRevealChoice::Round => Self::Round,
            GpRevealChoice::Song => Self::Song,
        }
    }
}

/// Choose the rules used by the game. The first name is the Discord label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum GpModeChoice {
    #[name = "Classic GP"]
    #[name = "classic"]
    Classic,
    #[name = "Wrong Prompt"]
    #[name = "wrong_prompt"]
    WrongPrompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpCategorySelection {
    Random,
    PickSeveral,
    Classic(GpCategory),
    WrongPrompt(WrongPromptTheme),
}

impl GpCategorySelection {
    pub(super) fn from_name(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some(choice) = GpCategoryChoice::from_name(value) {
            return Some(match GpCategory::from(choice) {
                GpCategory::Random => Self::Random,
                GpCategory::PickSeveral => Self::PickSeveral,
                category => Self::Classic(category),
            });
        }
        WrongPromptTheme::ALL
            .into_iter()
            .find(|theme| {
                theme.key().eq_ignore_ascii_case(value) || theme.label().eq_ignore_ascii_case(value)
            })
            .map(Self::WrongPrompt)
    }

    fn pool(self, mode: GpModeChoice) -> CrackedResult<Option<GpPromptPool>> {
        match (mode, self) {
            (_, Self::PickSeveral) => Ok(None),
            (GpModeChoice::Classic, Self::Random) => {
                Ok(Some(GpPromptPool::Classic(GpCategories::all())))
            },
            (GpModeChoice::WrongPrompt, Self::Random) => {
                Ok(Some(GpPromptPool::WrongPrompt(WrongPromptThemes::all())))
            },
            (GpModeChoice::Classic, Self::Classic(category)) => {
                Ok(GpCategories::new([category]).map(GpPromptPool::Classic))
            },
            (GpModeChoice::WrongPrompt, Self::WrongPrompt(theme)) => {
                Ok(WrongPromptThemes::new([theme]).map(GpPromptPool::WrongPrompt))
            },
            (GpModeChoice::Classic, Self::WrongPrompt(_)) => Err(CrackedError::Other(
                "Choose Wrong Prompt under mode to use that theme.",
            )),
            (GpModeChoice::WrongPrompt, Self::Classic(_)) => Err(CrackedError::Other(
                "Choose a Wrong Prompt theme, Random, or Pick several under category.",
            )),
        }
    }
}

// Discord's category options come from autocomplete. Keep a typed prefix
// argument so a leading round count such as `!gp start 5` can still skip it.
#[poise::async_trait]
impl poise::SlashArgument for GpCategorySelection {
    async fn extract(
        ctx: &::serenity::all::Context,
        interaction: &::serenity::all::CommandInteraction,
        value: &ResolvedValue<'_>,
    ) -> Result<Self, poise::SlashArgError> {
        let value = <String as poise::SlashArgument>::extract(ctx, interaction, value).await?;
        Self::from_name(&value).ok_or_else(|| {
            poise::SlashArgError::new_parse(
                CrackedError::Other("Choose a category from the suggestions.").into(),
                value,
            )
        })
    }

    fn create(
        option: ::serenity::all::CreateCommandOption<'_>,
    ) -> ::serenity::all::CreateCommandOption<'_> {
        <String as poise::SlashArgument>::create(option)
    }
}

#[poise::async_trait]
impl<'a> poise::PopArgument<'a> for GpCategorySelection {
    async fn pop_from(
        args: &'a str,
        attachment_index: usize,
        ctx: &::serenity::all::Context,
        msg: &::serenity::all::Message,
    ) -> Result<(&'a str, usize, Self), (Error, Option<String>)> {
        let (args, index, value) =
            <String as poise::PopArgument<'a>>::pop_from(args, attachment_index, ctx, msg).await?;
        let category = Self::from_name(&value).ok_or_else(|| {
            (
                CrackedError::Other("Choose a valid GP category.").into(),
                Some(value),
            )
        })?;
        Ok((args, index, category))
    }
}

pub(super) fn gp_category_suggestions(
    mode: GpModeChoice,
    partial: &str,
) -> Vec<(&'static str, &'static str)> {
    let mut suggestions = vec![
        (GpCategory::Random.display(), "random"),
        (GpCategory::PickSeveral.display(), "pick"),
    ];
    match mode {
        GpModeChoice::Classic => suggestions.extend(
            GpCategory::CATEGORIES
                .into_iter()
                .filter_map(|category| Some((category.display(), category.key()?))),
        ),
        GpModeChoice::WrongPrompt => suggestions.extend(
            WrongPromptTheme::ALL
                .into_iter()
                .map(|theme| (theme.label(), theme.key())),
        ),
    }
    let query = partial.trim().to_lowercase();
    suggestions
        .retain(|(label, key)| label.to_lowercase().contains(&query) || key.contains(&query));
    suggestions
}

async fn gp_category_autocomplete<'a>(
    ctx: poise::ApplicationContext<'_, crate::Data, Error>,
    partial: &'a str,
) -> CreateAutocompleteResponse<'a> {
    let mode = ctx
        .args
        .iter()
        .find_map(|option| match (option.name, &option.value) {
            ("mode", ResolvedValue::Integer(index)) => {
                GpModeChoice::from_index(usize::try_from(*index).ok()?)
            },
            _ => None,
        })
        .unwrap_or(GpModeChoice::Classic);
    let choices = gp_category_suggestions(mode, partial)
        .into_iter()
        .map(|(label, value)| AutocompleteChoice::new(label, value))
        .collect();
    CreateAutocompleteResponse::new().set_choices(Cow::Owned(choices))
}

/// Start a game in your voice channel: pick a category, rounds, and the submission timer.
// Live-Discord glue: it needs a poise Context (a voice channel, a picker the
// host answers). Its parts are tested; the choice conversions by
// `the_gp_choices_keep_their_names`.
#[cfg_attr(test, mutants::skip)]
#[allow(clippy::too_many_arguments)]
#[poise::command(
    rename = "start",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_start(
    ctx: Context<'_>,
    #[description = "Choose a category or theme for your mode. Defaults to Random."]
    #[autocomplete = "gp_category_autocomplete"]
    category: Option<GpCategorySelection>,
    #[description = "Number of rounds (Classic GP: 5, Wrong Prompt: 3)."]
    #[min = 1]
    #[max = 20]
    rounds: Option<u32>,
    #[description = "Seconds to submit each round (Classic GP: 180, Wrong Prompt: 90)."]
    #[min = 30]
    #[max = 600]
    timer: Option<u32>,
    #[description = "Play a clip of each song instead of all of it (default yes)."] clips: Option<
        bool,
    >,
    #[description = "Seconds into each song the clip starts (default 30, needs clips)."]
    #[min = 0]
    #[max = 120]
    clip_start: Option<u32>,
    #[description = "Seconds of each song to play (default 45, needs clips)."]
    #[min = 20]
    #[max = 300]
    clip_length: Option<u32>,
    #[description = "Name submitters only at the end of the round (default), or after each song."]
    reveal: Option<GpRevealChoice>,
    #[description = "Sum each round up in a results embed when it ends (default yes; always on with reveal:round)."]
    results: Option<bool>,
    #[description = "Play Classic GP (default) or Wrong Prompt."] mode: Option<GpModeChoice>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    if data.gp_is_active(guild_id) {
        return Err(CrackedError::GameAlreadyRunning.into());
    }
    let vc = ctx.author_vc().ok_or(CrackedError::NotConnected)?;
    let mode = mode.unwrap_or(GpModeChoice::Classic);
    let wrong_prompt = mode == GpModeChoice::WrongPrompt;
    let prompt_pool = match category.unwrap_or(GpCategorySelection::Random).pool(mode) {
        Ok(pool) => pool,
        Err(err) => {
            courier::reply_as(ctx, GpCard::Line(err.to_string()).into(), true).await?;
            return Ok(());
        },
    };
    if wrong_prompt {
        let player_count = gp_vc_members(ctx, vc).len();
        let refusal = match (reveal, results) {
            (Some(GpRevealChoice::Song), _) => {
                Some("Wrong Prompt reveals the picks together after voting. Choose the end-of-round reveal.")
            },
            (_, Some(false)) => Some("Wrong Prompt needs round results to reveal both prompts and the votes."),
            _ if player_count < GP_WRONG_PROMPT_MIN_PLAYERS => {
                Some("Wrong Prompt needs at least four people in your voice channel.")
            },
            _ if player_count > GP_MAX_PLAYERS => {
                Some("Wrong Prompt supports up to 25 players in your voice channel.")
            },
            _ => None,
        };
        if let Some(message) = refusal {
            courier::reply_as(ctx, GpCard::Line(message.into()).into(), true).await?;
            return Ok(());
        }
    }
    let prompt_pool = match prompt_pool {
        Some(pool) => pool,
        None => {
            let picker = match mode {
                GpModeChoice::Classic => GpPicker::Classic(Vec::new()),
                GpModeChoice::WrongPrompt => GpPicker::WrongPrompt(Vec::new()),
            };
            match gp_pick_prompts(ctx, picker).await? {
                Some(pool) => pool,
                None => return Ok(()),
            }
        },
    };
    // Looked up again: picking can take long enough to have left the channel.
    let vc = ctx.author_vc().ok_or(CrackedError::NotConnected)?;
    let host = ctx.author().id;
    let host_name = author_display_name(ctx).await;

    // Join (or reuse) the call, and make sure it is the host's channel.
    let call = get_call_or_join_author(ctx).await?;
    {
        let handler = call.lock().await;
        if let Some(chan) = handler.current_channel() {
            if chan.get() != vc.get() {
                return Err(CrackedError::WrongVoiceChannel.into());
            }
        }
    }

    let (default_rounds, default_timer_secs) = if wrong_prompt {
        (3, 90)
    } else {
        (GP_DEFAULT_ROUNDS, GP_DEFAULT_TIMER_SECS)
    };
    let rounds = rounds.unwrap_or(default_rounds).clamp(1, GP_MAX_ROUNDS) as usize;
    let timer_secs = timer
        .map(u64::from)
        .unwrap_or(default_timer_secs)
        .clamp(GP_MIN_TIMER_SECS, GP_MAX_TIMER_SECS);
    // The two dials only mean anything with clips on; off, they are ignored
    // rather than quietly turning clips back on for a host who asked for whole
    // songs.
    let clip = clips.unwrap_or(GP_DEFAULT_CLIPS).then(|| GpClip {
        start: Duration::from_secs(
            clip_start
                .map(u64::from)
                .unwrap_or(GP_DEFAULT_CLIP_START_SECS)
                .min(GP_MAX_CLIP_START_SECS),
        ),
        length: Duration::from_secs(
            clip_length
                .map(u64::from)
                .unwrap_or(GP_DEFAULT_CLIP_LENGTH_SECS)
                .clamp(GP_MIN_CLIP_LENGTH_SECS, GP_MAX_CLIP_LENGTH_SECS),
        ),
    });
    let reveal = reveal.map(GpReveal::from).unwrap_or_default();
    let round_results = results.unwrap_or(true) || reveal == GpReveal::Round;
    let category = prompt_pool.display();

    // Create the game first so the global TrackEndHandler ignores the End
    // event that stopping an existing queue fires.
    let opened = match prompt_pool {
        GpPromptPool::Classic(categories) => {
            let prompts = draw_prompts(&categories, rounds, &mut rand::rng());
            data.gp_start(
                guild_id,
                host,
                host_name,
                vc,
                ctx.channel_id(),
                categories,
                prompts,
                timer_secs,
                clip,
                reveal,
                round_results,
                now(),
            )?
        },
        GpPromptPool::WrongPrompt(themes) => {
            let mut pairs = draw_wrong_prompts(&themes, &mut rand::rng());
            let rounds = rounds.min(pairs.len());
            // Game state takes each successive pair from the back of the deck.
            pairs.reverse();
            data.gp_data().gp_start_wrong_prompt(
                guild_id,
                host,
                host_name,
                vc,
                ctx.channel_id(),
                pairs,
                rounds,
                timer_secs,
                clip,
                now(),
            )?
        },
    };
    // `data.gp_start` above claimed the lease, so the game owns playback by the
    // time we reach here and this must lock as `Game`; `Free` would be refused
    // and the previous queue would keep playing underneath the new game.
    // 🪤 Do NOT reorder the claim below this call to make the guild `Free`
    // here: creating the game first is load-bearing, for the reason the comment
    // above `data.gp_start` gives. Locking as `Game` cannot be refused (`as_`
    // matches both `Free` and `Game`), so this `?` cannot orphan the game that
    // was just created.
    let guard = data
        .lock_queue(
            guild_id,
            PlaybackOwner::Game,
            crate::music::audit::Actor::from_ctx(&ctx),
        )
        .await?;
    let cleared_queue = {
        let handler = call.lock().await;
        let non_empty = !handler.queue().is_empty();
        if non_empty {
            #[expect(
                clippy::disallowed_methods,
                reason = "/gp start stops any leftover queue as the game owner before the first round"
            )]
            stop_queue(&guard, &handler);
        }
        non_empty
    };
    // Released before the Discord round trip below -- `stop_queue` fires `End`
    // inline on songbird's event task, which must not wait out a send.
    drop(guard);

    let acknowledgement = ctx
        .send_reply(
            GpCard::Started {
                category,
                rounds: opened.total_rounds,
                timer_secs,
                clip,
                reveal,
                round_results,
                cleared_queue,
            }
            .into(),
            true,
        )
        .await;
    if let Err(e) = acknowledgement {
        if !wrong_prompt {
            return Err(e.into());
        }
        tracing::warn!("gp: acknowledging Wrong Prompt start in {guild_id}: {e}");
    }

    let pb = gp_playback(ctx, call, guild_id);
    gp_open_round(&pb, opened).await
}

/// Secretly submit your song for the current prompt (link or search). Resubmitting replaces it.
#[poise::command(
    rename = "submit",
    category = "Games",
    slash_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_submit(
    ctx: Context<'_>,
    #[description = "song link or search query."] query: String,
) -> Result<(), Error> {
    // Errors are answered here, ephemerally, rather than through the framework's
    // public error reply.
    let msg = match gp_submit_internal(ctx, query).await {
        Ok(m) => m,
        Err(e) => refuse_gp("submit", ctx.author().id, ctx.guild_id(), e),
    };
    ctx.send_message(SendMessageParams::new(msg).with_ephemeral(true))
        .await?;
    Ok(())
}

/// Answer a refused `/gp` command, and leave the operator a trace of it (#468).
///
/// The reply stays ephemeral at every call site -- a submission is secret, and
/// a vote names the voter. What was missing is the server-side record: a
/// refusal used to leave nothing in the log. 🔑 Logging and building the reply
/// in one function means a new refusal site gets both or neither.
pub(in crate::commands::music::gp) fn refuse_gp(
    command: &'static str,
    user_id: UserId,
    guild_id: Option<GuildId>,
    err: CrackedError,
) -> CrackedMessage {
    tracing::warn!(command, %user_id, ?guild_id, error = %err, "gp: command refused");
    CrackedMessage::from(err)
}

/// Resolve a Spotify link for `/gp submit`, which takes exactly one song.
///
/// An album or playlist is refused rather than quietly reduced to its first
/// track: *which* song a player submits is the whole game, so choosing one for
/// them would replace their move with ours and they would never know.
async fn gp_spotify_query(url: &str) -> CrackedResult<QueryType> {
    let resolution = sleevenote::resolve_spotify(url).await?;
    if resolution.media_type.is_collection() {
        return Err(CrackedError::Other(SPOTIFY_GP_ONE_SONG));
    }
    let query = resolution
        .queries()
        .into_iter()
        .next()
        .ok_or(CrackedError::Other(SPOTIFY_NOTHING_PLAYABLE))?;
    Ok(QueryType::Keywords(query))
}

pub async fn gp_submit_internal(ctx: Context<'_>, query: String) -> CrackedResult<CrackedMessage> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_in_game_vc(ctx, guild_id)?;
    let submission_generation = data
        .gp_data()
        .gp_submission_generation(guild_id, ctx.author().id)?;
    let query = query.trim();
    if query.is_empty() {
        return Err(CrackedError::NoQuery);
    }
    let query_type = match QueryType::from_str(query).map_err(CrackedError::TrackResolveError)? {
        QueryType::SpotifyLink(url) => {
            // Resolving a Spotify link is an HTTP round trip that takes ten
            // seconds or more on a cold cache, well past Discord's
            // three-second interaction deadline -- so defer before doing any
            // of it, or the command fails before the answer exists. Only this
            // path defers: a search or a YouTube link answers fast enough that
            // a "thinking" state would be a downgrade.
            ctx.defer_ephemeral().await?;
            gp_spotify_query(&url).await?
        },
        other => other,
    };
    let track = data
        .ct_client
        .resolve_track(query_type)
        .await
        .map_err(CrackedError::TrackFail)?;
    let name = author_display_name(ctx).await;
    let title = track.get_title();
    if submission_generation.is_some() && ctx.author_vc() != Some(game_vc) {
        return Err(CrackedError::NotInGameVoiceChannel);
    }
    let vc_members = gp_vc_members(ctx, game_vc);
    let outcome = data.gp_submit_if(
        guild_id,
        ctx.author().id,
        name,
        track,
        &vc_members,
        submission_generation,
    )?;

    // Take the call first: closing commits the shuffle and moves the game to
    // `Playing`, so discovering there is nothing to play on afterwards would
    // strand the round with nothing ever enqueued to advance it.
    if outcome.everyone_in {
        // 🪤 `connected_call`, not `Songbird::get` (#507). A Call left behind by
        // a join that never completed satisfied `get`, so the window closed and
        // the round was enqueued into a driver connected to nothing -- the
        // "Queued, then silence" #499 fixed on /play. Not connected, the window
        // now stays open rather than closing onto nothing.
        if let Some(call) = crate::commands::connected_call(&data.songbird, guild_id, None).await {
            if let Some(closed) =
                data.gp_close_window_if(guild_id, outcome.generation, &mut rand::rng(), now())
            {
                let pb = gp_playback(ctx, call, guild_id);
                // Off this task so the ephemeral confirmation isn't held up.
                tokio::spawn(async move {
                    if let Err(e) = gp_after_close(pb, closed).await {
                        tracing::warn!("gp: closing window in {guild_id}: {e}");
                    }
                });
            }
        } else {
            tracing::warn!(
                "gp: everyone is in for {guild_id} but the bot is not connected; \
                 leaving the submission window open"
            );
        }
    }
    Ok(CrackedMessage::GpSubmitted {
        title,
        replaced: outcome.replaced,
        submitted: outcome.submitted,
        of: vc_members.len(),
    })
}

/// Close submissions early and start playing this round (host only).
#[poise::command(
    rename = "close",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_close(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    gp_require_player(ctx, guild_id)?;
    // Take the call *before* closing: `gp_close_window` commits the shuffle and
    // moves the game to `Playing`, and bailing after that would leave the round
    // closed with nothing ever enqueued to advance it.
    let call = crate::commands::connected_call(&data.songbird, guild_id, None)
        .await
        .ok_or(CrackedError::NotConnected)?;
    let closed = data.gp_close_window(guild_id, ctx.author().id, &mut rand::rng(), now())?;
    if let Err(e) = ctx
        .send_reply(
            CrackedMessage::GpWindowClosed {
                count: closed.count,
            },
            true,
        )
        .await
    {
        tracing::warn!("gp: acknowledging the close in {guild_id}: {e}");
    }
    gp_after_close(gp_playback(ctx, call, guild_id), closed).await
}

/// End the current song early (host only).
#[poise::command(
    rename = "skip",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_skip(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    gp_require_player(ctx, guild_id)?;
    {
        let game = data
            .gp_games
            .get(&guild_id)
            .ok_or(CrackedError::NoGameInProgress)?;
        if game.host != ctx.author().id {
            return Err(CrackedError::NotGameHost.into());
        }
        if game.phase != GpPhase::Playing {
            return Err(CrackedError::GameNotPlaying.into());
        }
    }
    let call = crate::commands::connected_call(&data.songbird, guild_id, None)
        .await
        .ok_or(CrackedError::NotConnected)?;
    {
        // Locking as `Game` succeeds here because this guild's game already
        // owns playback and has not released it (see
        // `the_owner_can_still_lock_its_own_queue` in lease.rs); `Free` would
        // be refused and the skip would silently do nothing.
        let guard = data
            .lock_queue(
                guild_id,
                PlaybackOwner::Game,
                crate::music::audit::Actor::from_ctx(&ctx),
            )
            .await?;
        let handler = call.lock().await;
        if handler.queue().is_empty() {
            return Err(CrackedError::NothingPlaying.into());
        }
        // stop() fires TrackEvent::End, which is what advances the game.
        #[expect(
            clippy::disallowed_methods,
            reason = "/gp skip is the game owner moving its own round on"
        )]
        force_skip_top_track(&guard, &handler).await?;
    }
    ctx.send_reply(CrackedMessage::GpRoundSkipped, true).await?;
    Ok(())
}

/// What a vote command says: `mine` goes to the voter alone, `room` -- if there
/// is anything the room should hear -- to the game's channel, naming nobody.
type GpVoteAnswer = (CrackedMessage, Option<CrackedMessage>);

/// Answer a vote. The voter's confirmation is ephemeral, the same as
/// `/gp submit`'s: a slash reply lands under "*name* used `/gp voteskip`", and
/// who wants a song gone is nobody's business in a game whose whole premise is
/// that people submitted something embarrassing. What the room is told goes to
/// the game's channel as a plain message, so the last voter is not named on
/// that either.
async fn gp_answer_vote(ctx: Context<'_>, (mine, room): GpVoteAnswer) -> Result<(), Error> {
    if ctx.is_prefix() {
        // `ephemeral` is an interaction flag: a prefix invocation gets a public
        // reply, and "you already voted to skip this" names the voter as surely
        // as the vote would have. Answer by DM instead, and if that cannot be
        // delivered say nothing in the channel -- the room's line below still
        // says the vote counted. (The `!gp voteskip` message itself is public;
        // the slash form is the one that keeps a vote to yourself.)
        let dm = ctx
            .author()
            .create_dm_channel(&ctx)
            .await
            .map(|c| c.id.widen());
        let transport = DiscordTransport::of(ctx.serenity_context());
        let sent: Result<(), String> = match dm {
            Ok(dm) => {
                courier::post_message(&transport, dm, &gp_rendered(GpCard::Line(mine.to_string())))
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            Err(e) => Err(e.to_string()),
        };
        if let Err(e) = sent {
            tracing::warn!(
                "gp: answering a prefix vote by DM in {:?}: {e}",
                ctx.guild_id()
            );
        }
    } else {
        ctx.send_message(SendMessageParams::new(mine).with_ephemeral(true))
            .await?;
    }
    let Some(room) = room else {
        return Ok(());
    };
    // The vote may have ended the game's last song; then there is no game and
    // nothing to tell anyone.
    let Some(channel) = ctx
        .guild_id()
        .and_then(|guild_id| ctx.data().gp_text_channel(guild_id))
    else {
        return Ok(());
    };
    courier::post_message(
        &DiscordTransport::of(ctx.serenity_context()),
        channel,
        &gp_rendered(GpCard::Line(room.to_string())),
    )
    .await?;
    Ok(())
}

/// Vote to end the current song early -- a majority of the voice channel ends it.
#[poise::command(
    rename = "voteskip",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_voteskip(ctx: Context<'_>) -> Result<(), Error> {
    // Errors are answered here, ephemerally, rather than through the framework's
    // public error reply: "you already voted to skip this" names the voter as
    // surely as the vote would have.
    let answer = match gp_voteskip_internal(ctx).await {
        Ok(a) => a,
        Err(e) => (
            refuse_gp("voteskip", ctx.author().id, ctx.guild_id(), e),
            None,
        ),
    };
    gp_answer_vote(ctx, answer).await
}

async fn gp_voteskip_internal(ctx: Context<'_>) -> CrackedResult<GpVoteAnswer> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_player(ctx, guild_id)?;
    let vc_members = gp_vc_members(ctx, game_vc);
    let name = author_display_name(ctx).await;
    let outcome = data.gp_vote_skip(guild_id, ctx.author().id, name, &vc_members)?;
    let answer = match outcome {
        GpVoteSkipOutcome::Counted { votes, needed } => (
            CrackedMessage::GpVoteSkipCounted { votes, needed },
            Some(CrackedMessage::GpVoteSkipRoom { needed }),
        ),
        GpVoteSkipOutcome::Passed => (
            CrackedMessage::GpVoteSkipCarried,
            Some(CrackedMessage::GpVoteSkipPassed),
        ),
        GpVoteSkipOutcome::OwnSong => (
            CrackedMessage::GpVoteSkipOwnSong,
            Some(CrackedMessage::GpVoteSkipPulled),
        ),
    };
    if !matches!(outcome, GpVoteSkipOutcome::Counted { .. }) {
        let call = crate::commands::connected_call(&data.songbird, guild_id, None)
            .await
            .ok_or(CrackedError::NotConnected)?;
        // Locking as `Game` succeeds here because this guild's game already
        // owns playback and has not released it (see
        // `the_owner_can_still_lock_its_own_queue` in lease.rs); `Free` would
        // be refused and the skip would silently do nothing.
        let guard = data
            .lock_queue(
                guild_id,
                PlaybackOwner::Game,
                crate::music::audit::Actor::from_ctx(&ctx),
            )
            .await?;
        let handler = call.lock().await;
        if handler.queue().is_empty() {
            return Err(CrackedError::NothingPlaying);
        }
        // stop() fires TrackEvent::End, which is what advances the game.
        #[expect(
            clippy::disallowed_methods,
            reason = "a /gp vote-skip is the game owner moving its own round on"
        )]
        force_skip_top_track(&guard, &handler).await?;
    }
    Ok(answer)
}

/// Vote to hear the whole song instead of just the clip -- a majority lets it run.
#[poise::command(
    rename = "votefull",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    ephemeral,
    check = "cmd_check_music"
)]
pub async fn gp_votefull(ctx: Context<'_>) -> Result<(), Error> {
    // Same split as `/gp voteskip`, for the same reason: one pattern, not two.
    let answer = match gp_votefull_internal(ctx).await {
        Ok(a) => a,
        Err(e) => (
            refuse_gp("votefull", ctx.author().id, ctx.guild_id(), e),
            None,
        ),
    };
    gp_answer_vote(ctx, answer).await
}

async fn gp_votefull_internal(ctx: Context<'_>) -> CrackedResult<GpVoteAnswer> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let game_vc = gp_require_player(ctx, guild_id)?;
    let vc_members = gp_vc_members(ctx, game_vc);
    let name = author_display_name(ctx).await;
    Ok(
        match data.gp_vote_full(guild_id, ctx.author().id, name, &vc_members)? {
            GpVoteFullOutcome::Counted { votes, needed } => (
                CrackedMessage::GpVoteFullCounted { votes, needed },
                Some(CrackedMessage::GpVoteFullRoom { needed }),
            ),
            // Nothing to do to the track: the clip timer checks `play_full` before
            // it stops anything, so letting it run on is simply not stopping it.
            GpVoteFullOutcome::Passed => (
                CrackedMessage::GpVoteFullCarried,
                Some(CrackedMessage::GpVoteFullPassed),
            ),
            // Already carried by an earlier vote: the room heard about it then.
            GpVoteFullOutcome::AlreadyFull => (CrackedMessage::GpVoteFullAlready, None),
        },
    )
}

/// The prompt, who has submitted or guessed, likes, and the scores so far.
#[poise::command(
    rename = "status",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_status(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    gp_require_player(ctx, guild_id)?;
    let status = ctx.data().gp_status(guild_id)?;
    courier::reply_as(ctx, GpCard::Status((&status).into()).into(), false).await?;
    Ok(())
}

/// Abort the game (host, or anyone who can manage the server).
#[poise::command(
    rename = "end",
    category = "Games",
    slash_command,
    prefix_command,
    guild_only,
    check = "cmd_check_music"
)]
pub async fn gp_end(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or(CrackedError::NoGuildId)?;
    let data = ctx.data();
    let is_admin = ctx
        .author_permissions()
        .await
        .map(|p| p.manage_guild())
        .unwrap_or(false);
    // Park and stop, but do not remove: `stop()` only queues the `End`, so removing
    // here would beat it to the map and hand the event to autoplay. The global
    // track-end handler collects the parked game when the `End` actually lands.
    let game = data.gp_park_for_end(guild_id, ctx.author().id, is_admin)?;
    // Raw `get` on purpose (#507), unlike `/gp submit`. This is cleanup, and
    // stopping the queue must reach any registered Call, connected or not:
    // songbird reuses a guild's Call on the next join, so tracks left in a
    // stranded one would start playing in whatever comes next.
    #[expect(
        clippy::disallowed_methods,
        reason = "raw Songbird::get on purpose (#507: cleanup reaches any registered Call), and the game owner stopping its own queue"
    )]
    let was_playing = match data.songbird.get(guild_id) {
        Some(call) => {
            // `gp_park_for_end` only sets a flag -- the game stays in the map,
            // and with it the lease, until `gp_remove_if_parked` collects it in
            // the track-end handler. So the game still owns playback here and
            // this locks as `Game`; `Free` would be refused and the queue would
            // never stop, leaving the parked game with no `End` to collect it.
            let guard = data
                .lock_queue(
                    guild_id,
                    PlaybackOwner::Game,
                    crate::music::audit::Actor::from_ctx(&ctx),
                )
                .await?;
            let handler = call.lock().await;
            let playing = !handler.queue().is_empty();
            stop_queue(&guard, &handler);
            playing
            // Both locks drop with this arm, before the replies below.
        },
        None => false,
    };
    if was_playing {
        // If that `End` never reaches the handler, the parked game would outlive
        // the command and keep the guild's music commands blocked.
        gp_spawn_park_backstop((*data).clone(), guild_id, game.started_at);
    } else {
        // Nothing was playing, so no `End` is coming to collect it.
        data.gp_remove(guild_id);
    }
    if game.wrong_prompt.is_some() {
        let controls = if game.phase == GpPhase::Submitting {
            game.rounds
                .get(game.current_round)
                .and_then(|round| round.prompt_message)
        } else {
            game.wrong_prompt_window().and_then(|window| window.message)
        };
        if let Some((channel, message)) = controls {
            let transport = DiscordTransport::of(ctx.serenity_context());
            crack_gp::gp_persist::take_down_components(&transport, guild_id, channel, message)
                .await;
        }
    }
    let by = author_display_name(ctx).await;
    ctx.send_reply(CrackedMessage::GpEnded { by }, true).await?;
    let nothing_played = game.phase == GpPhase::Submitting && game.current_round == 0;
    if !nothing_played {
        courier::reply_as(
            ctx,
            GpCard::Scoreboard {
                scores: user_refs(&game.sorted_scores()),
                title: GP_SCOREBOARD,
                lead: None,
            }
            .into(),
            false,
        )
        .await?;
    }
    Ok(())
}

/// Collect the game `/gp end` parked, if its `End` has not done so within
/// [`GP_PARK_GRACE_SECS`]. Only that game: two `/gp end`s about ten seconds
/// apart put this inside the next game's parked window (#423).
pub(super) fn gp_spawn_park_backstop(data: crate::Data, guild_id: GuildId, started_at: i64) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(GP_PARK_GRACE_SECS)).await;
        if data.gp_collect_parked(guild_id, started_at) {
            tracing::warn!("gp: parked game in {guild_id} was never collected, removing");
        }
    });
}
