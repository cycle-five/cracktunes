use super::playback::{
    gp_abort, gp_component_vc_check, gp_edit_or_post, gp_follow, gp_open_round, gp_send, GpPlayback,
};
use crate::{
    errors::CrackedError,
    messaging::{
        courier,
        render::RenderCx,
        transport::{DiscordPress, DiscordTransport},
    },
    CrackedResult, Data, Error,
};
use crack_gp::gp::{
    now, wrong_prompt_results, GpCard, GpNext, GpPhase, GpWindowOpened, GpWrongPromptWindow,
    WrongPromptCard, GP_CUSTOM_ID_PREFIX, GP_REVEAL_PAUSE_SECS,
};
use serenity::all::{ComponentInteraction, ComponentInteractionDataKind, Context, GuildId};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

#[derive(Clone, Copy)]
pub(super) enum WrongPromptAction {
    Prompt,
    Ballot,
    Vote,
}

pub(super) fn component_id(action: WrongPromptAction, guild: GuildId, generation: u64) -> String {
    let action = match action {
        WrongPromptAction::Prompt => "prompt",
        WrongPromptAction::Ballot => "ballot",
        WrongPromptAction::Vote => "vote",
    };
    format!("{GP_CUSTOM_ID_PREFIX}wp:{action}:{guild}:{generation}")
}

pub(super) fn parse_component(id: &str) -> Option<(WrongPromptAction, GuildId, u64)> {
    let mut fields = id.strip_prefix(GP_CUSTOM_ID_PREFIX)?.split(':');
    if fields.next()? != "wp" {
        return None;
    }
    let action = match fields.next()? {
        "prompt" => WrongPromptAction::Prompt,
        "ballot" => WrongPromptAction::Ballot,
        "vote" => WrongPromptAction::Vote,
        _ => return None,
    };
    let guild = fields.next()?.parse::<u64>().ok().filter(|id| *id != 0)?;
    let generation = fields.next()?.parse().ok()?;
    fields
        .next()
        .is_none()
        .then_some((action, GuildId::new(guild), generation))
}

pub(super) async fn handle_component(
    data: &Data,
    ctx: &Context,
    interaction: &ComponentInteraction,
    action: WrongPromptAction,
    guild: GuildId,
    generation: u64,
) -> Result<(), Error> {
    if interaction.guild_id != Some(guild) {
        return Ok(());
    }
    let result = gp_component_vc_check(data, ctx, interaction, guild)
        .and_then(|()| answer(data, interaction, action, guild, generation));
    let (card, complete) = match result {
        Ok(answer) => answer,
        Err(err) => (GpCard::Line(err.to_string()), false),
    };

    if complete {
        let data = Arc::new(data.clone());
        let transport = Arc::new(DiscordTransport::of(ctx));
        let cache = Some(ctx.cache.clone());
        tokio::spawn(async move {
            if let Some(call) = crate::commands::connected_call(&data.songbird, guild, None).await {
                let pb = GpPlayback {
                    data,
                    transport,
                    cache,
                    call,
                    guild_id: guild,
                };
                finish_ballot(pb, generation, now()).await;
            }
        });
    }

    let press = DiscordPress {
        http: &ctx.http,
        interaction,
    };
    courier::respond(&press, &card.into(), &RenderCx::now(), true).await?;
    Ok(())
}

fn answer(
    data: &Data,
    interaction: &ComponentInteraction,
    action: WrongPromptAction,
    guild: GuildId,
    generation: u64,
) -> CrackedResult<(GpCard, bool)> {
    let gp = data.gp_data();
    let user = interaction.user.id;
    let card = match (action, &interaction.data.kind) {
        (WrongPromptAction::Prompt, ComponentInteractionDataKind::Button) => {
            WrongPromptCard::PrivatePrompt {
                prompt: gp.gp_wrong_prompt_card(guild, generation, user)?,
            }
        },
        (WrongPromptAction::Ballot, ComponentInteractionDataKind::Button) => {
            WrongPromptCard::Ballot {
                select_id: component_id(WrongPromptAction::Vote, guild, generation),
                track_indices: gp.gp_wrong_prompt_choices(guild, generation, user, now())?,
            }
        },
        (WrongPromptAction::Vote, ComponentInteractionDataKind::StringSelect { values }) => {
            let value = values
                .first()
                .filter(|_| values.len() == 1)
                .ok_or(CrackedError::StaleRound)?;
            let clip_idx = value.parse().map_err(|_| CrackedError::StaleRound)?;
            let complete = gp.gp_wrong_prompt_vote(guild, generation, user, clip_idx, now())?;
            return Ok((GpCard::Line("Your vote is in.".into()), complete));
        },
        _ => return Err(CrackedError::StaleRound),
    };
    Ok((GpCard::WrongPrompt(card), false))
}

fn seconds_until(deadline: i64) -> Duration {
    Duration::from_secs(deadline.saturating_sub(now()).max(0) as u64)
}

pub async fn gp_resume_wrong_prompt_submission(
    pb: GpPlayback,
    remaining: u64,
) -> Result<(), Error> {
    let Some(opened) = pb.data.gp_games.get(&pb.guild_id).and_then(|game| {
        if game.phase != GpPhase::Submitting || game.wrong_prompt.is_none() {
            return None;
        }
        Some(GpWindowOpened {
            round_idx: game.current_round,
            total_rounds: game.rounds.len(),
            prompt: String::new(),
            category: None,
            closes_at: game.rounds.get(game.current_round)?.closes_at?,
            timer_secs: remaining,
            generation: game.generation,
            text_channel: game.text_channel,
            wrong_prompt: true,
        })
    }) else {
        return Ok(());
    };
    gp_open_round(&pb, opened).await
}

pub async fn gp_resume_wrong_prompt(
    pb: GpPlayback,
    window: GpWrongPromptWindow,
    phase: GpPhase,
) -> Result<(), Error> {
    match phase {
        GpPhase::Discussing => {
            let remaining = seconds_until(window.closes_at);
            if !remaining.is_zero() {
                if let Err(err) = gp_send(
                    &pb,
                    window.text_channel,
                    GpCard::WrongPrompt(WrongPromptCard::Discussion {
                        round_idx: window.round_idx,
                        total_rounds: window.total_rounds,
                        vote_at: window.closes_at,
                    }),
                )
                .await
                {
                    tracing::warn!(
                        "gp: posting the Wrong Prompt discussion in {}: {err}",
                        pb.guild_id
                    );
                }
            }
            tokio::spawn(async move {
                tokio::time::sleep(seconds_until(window.closes_at)).await;
                let guild = pb.guild_id;
                let ballot =
                    pb.data
                        .gp_data()
                        .gp_open_wrong_prompt_ballot(guild, window.generation, now());
                if let Some(ballot) = ballot {
                    if let Err(err) = post_ballot(pb, ballot).await {
                        tracing::warn!("gp: opening the Wrong Prompt ballot in {guild}: {err}");
                    }
                }
            });
        },
        GpPhase::Voting => post_ballot(pb, window).await?,
        _ => {},
    }
    Ok(())
}

async fn post_ballot(pb: GpPlayback, window: GpWrongPromptWindow) -> Result<(), Error> {
    let guild = pb.guild_id;
    if seconds_until(window.closes_at).is_zero() {
        tokio::spawn(finish_ballot(pb, window.generation, window.closes_at));
        return Ok(());
    }
    if let Some((channel, message)) = window.message {
        // Remove the pre-restart button before replacing it.
        let _ = pb.transport.clear_components(channel, message).await;
    }
    let card = GpCard::WrongPrompt(WrongPromptCard::Voting {
        round_idx: window.round_idx,
        total_rounds: window.total_rounds,
        closes_at: window.closes_at,
        ballot_id: Some(component_id(
            WrongPromptAction::Ballot,
            guild,
            window.generation,
        )),
    });
    let message = match gp_send(&pb, window.text_channel, card).await {
        Ok(message) => message,
        Err(err) => {
            gp_abort(
                &pb,
                window.text_channel,
                &format!("posting the Wrong Prompt ballot: {err}"),
            )
            .await;
            return Ok(());
        },
    };
    if pb.data.gp_data().gp_set_wrong_prompt_ballot_message(
        guild,
        window.generation,
        (window.text_channel, message),
    ) {
        tokio::spawn(finish_ballot(pb, window.generation, window.closes_at));
    } else {
        let _ = pb
            .transport
            .clear_components(window.text_channel, message)
            .await;
    }
    Ok(())
}

// The timer goes through the same transition as the last vote. Its interface
// erases the future type because following a retry can open another round.
fn finish_ballot(
    pb: GpPlayback,
    generation: u64,
    deadline: i64,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        tokio::time::sleep(seconds_until(deadline)).await;
        if let Err(err) = finish_ballot_now(&pb, generation).await {
            tracing::warn!(
                "gp: closing the Wrong Prompt ballot in {}: {err}",
                pb.guild_id
            );
        }
    })
}

async fn finish_ballot_now(pb: &GpPlayback, generation: u64) -> Result<(), Error> {
    let guild = pb.guild_id;
    let Some(text_channel) = pb.data.gp_text_channel(guild) else {
        return Ok(());
    };
    let Some(finished) = pb
        .data
        .gp_data()
        .gp_finish_wrong_prompt_ballot(guild, generation, now())
    else {
        return Ok(());
    };
    if let Some((channel, message)) = finished.message {
        let _ = pb.transport.clear_components(channel, message).await;
    }
    if let Some(round) = &finished.result {
        if let Some(view) = wrong_prompt_results(round) {
            let card = GpCard::WrongPrompt(WrongPromptCard::Results(view));
            match gp_send(pb, text_channel, card).await {
                Ok(_) => pb.data.gp_data().gp_mark_results_posted_if(
                    guild,
                    round.round_idx,
                    finished.generation,
                ),
                Err(err) => tracing::warn!(
                    "gp: posting Wrong Prompt round {} results in {guild}: {err}",
                    round.round_idx + 1
                ),
            }
        }
    }
    if let GpNext::Finished(scores) = finished.next {
        tokio::time::sleep(Duration::from_secs(GP_REVEAL_PAUSE_SECS)).await;
        if pb
            .data
            .gp_data()
            .gp_remove_finished(guild, finished.generation)
            .is_some()
        {
            super::playback::gp_post_final_scores(pb, text_channel, scores, true).await?;
        }
        return Ok(());
    }
    gp_follow(
        pb.clone(),
        finished.next,
        text_channel,
        finished.result.is_some(),
    )
    .await
}

pub(super) async fn close_prompt(pb: &GpPlayback, closed: &crack_gp::gp::GpWindowClosed) {
    let card = GpCard::WrongPrompt(WrongPromptCard::Opening {
        round_idx: closed.round_idx,
        total_rounds: closed.total_rounds,
        closes_at: 0,
        prompt_id: None,
    });
    gp_edit_or_post(pb, closed.prompt_message, closed.text_channel, card).await;
}

pub(super) fn prompt_card(pb: &GpPlayback, opened: &GpWindowOpened) -> GpCard {
    GpCard::WrongPrompt(WrongPromptCard::Opening {
        round_idx: opened.round_idx,
        total_rounds: opened.total_rounds,
        closes_at: opened.closes_at,
        prompt_id: Some(component_id(
            WrongPromptAction::Prompt,
            pb.guild_id,
            opened.generation,
        )),
    })
}
