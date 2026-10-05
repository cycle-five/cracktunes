//! Skips: what plays changes, so they settle `NowPlaying`.
use super::*;
use crate::{
    connection::get_voice_channel_for_user,
    messaging::message::CrackedMessage,
    music::{
        queue::{drain_after_current, force_skip_top_track},
        remote::{summarize, TrackSummary},
    },
};
use serenity::all::{GuildId, UserId};

/// What a skip did: what plays now, and how many tracks went.
#[derive(Debug)]
pub struct Skipped {
    pub now: Option<TrackSummary>,
    pub count: usize,
}

/// A vote: counted, or it carried and the track was skipped.
#[derive(Debug)]
pub enum Vote {
    Voted { missing: usize },
    Skipped(Skipped),
}

impl Skipped {
    /// What `/skip` always said: the next track, or "skipped" / "skipped all".
    pub fn message(&self) -> CrackedMessage {
        match &self.now {
            Some(t) => CrackedMessage::SkipTo {
                title: t.title.clone().unwrap_or_default(),
                url: t.url.clone().unwrap_or_default(),
            },
            None if self.count > 1 => CrackedMessage::SkipAll,
            None => CrackedMessage::Skip,
        }
    }
}

/// Skip `count` tracks (the playing one included). With `expect`, only if the
/// playing track is still that id.
pub async fn skip(
    cx: &OpCx,
    count: usize,
    expect: Option<uuid::Uuid>,
) -> Result<Done<Skipped>, OpRefused> {
    let (g, call) = begin(cx).await?;
    skip_on(&g, &call, count, expect).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
pub(crate) async fn skip_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    count: usize,
    expect: Option<uuid::Uuid>,
) -> Result<Done<Skipped>, OpRefused> {
    let next = {
        let handler = call.lock().await;
        let queue = handler.queue();
        let current = queue.current().ok_or(OpRefused::NothingPlaying)?;
        if expect.is_some_and(|id| id != current.uuid()) {
            return Err(OpRefused::Stale);
        }
        let count = count.max(1).min(queue.len());
        drain_after_current(g, &handler, count - 1);
        // `force_skip_top_track` has no `Err` path today; a failure would be
        // songbird refusing the skip.
        force_skip_top_track(g, &handler)
            .await
            .map_err(|_| OpRefused::Failed(Failure::Skip))?;
        (handler.queue().current(), count)
    };
    // 🔑 The Call lock is released: `summarize` reads track metadata.
    let (next, count) = next;
    let now = match next {
        Some(h) => summarize(std::slice::from_ref(&h)).await.pop(),
        None => None,
    };
    Ok(Done {
        outcome: Skipped { now, count },
        settle: Settle::NowPlaying,
        call: Some(call.clone()),
    })
}

/// Votes needed: half the listeners in the bot's channel, as /voteskip always did.
pub(crate) fn skip_threshold(in_channel: usize) -> usize {
    in_channel / 2
}

/// Count `voter`; `Err(missing)` while the votes fall short of `threshold`.
pub(crate) async fn cast_vote(
    data: &crate::Data,
    guild_id: GuildId,
    voter: UserId,
    threshold: usize,
) -> Result<(), usize> {
    let mut map = data.guild_cache_map.lock().await;
    let votes = &mut map.entry(guild_id).or_default().current_skip_votes;
    votes.insert(voter);
    if votes.len() >= threshold {
        Ok(())
    } else {
        Err(threshold - votes.len())
    }
}

/// Vote to skip. Clearing the votes after a skip is the track-end handler's
/// job (`forget_skip_votes`).
pub async fn voteskip(cx: &OpCx, voter: UserId) -> Result<Done<Vote>, OpRefused> {
    let in_channel = {
        let guild = cx.cache.guild(cx.guild_id).ok_or(OpRefused::NotConnected)?;
        let bot_channel = get_voice_channel_for_user(&guild, &cx.cache.current_user().id)
            .map_err(|_| OpRefused::NotConnected)?;
        guild
            .voice_states
            .iter()
            .filter(|v| v.channel_id == Some(bot_channel))
            .count()
    };
    let (g, call) = begin(cx).await?;
    if call.lock().await.queue().is_empty() {
        return Err(OpRefused::NothingPlaying);
    }
    match cast_vote(&cx.data, cx.guild_id, voter, skip_threshold(in_channel)).await {
        Ok(()) => Ok(skip_on(&g, &call, 1, None).await?.map(Vote::Skipped)),
        Err(missing) => Ok(Done {
            outcome: Vote::Voted { missing },
            settle: Settle::Nothing,
            call: Some(call),
        }),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::{audit::Action, ops::test_support::*};

    #[tokio::test]
    async fn skip_one_plays_the_next_and_settles_now_playing() {
        let (data, call, ids, mut rx) = queue_of(3).await;
        let g = guard(&data).await;
        let done = skip_on(&g, &call, 1, None).await.unwrap();
        assert_eq!(*done.settle(), Settle::NowPlaying);
        assert_eq!(done.outcome().now.as_ref().map(|t| t.id), Some(ids[1]));
        assert_eq!(ids_of(&call).await, ids[1..].to_vec());
        assert!(matches!(
            recorded(&mut rx).as_slice(),
            [Action::Skip { track: Some(_) }]
        ));
    }

    #[tokio::test]
    async fn skip_n_drains_then_skips() {
        let (data, call, ids, _) = queue_of(5).await;
        let g = guard(&data).await;
        let _ = skip_on(&g, &call, 3, None).await.unwrap();
        assert_eq!(ids_of(&call).await, ids[3..].to_vec());
    }

    #[tokio::test]
    async fn skipping_more_than_queued_empties_it() {
        let (data, call, _, _) = queue_of(2).await;
        let g = guard(&data).await;
        let done = skip_on(&g, &call, 9, None).await.unwrap();
        assert!(ids_of(&call).await.is_empty());
        assert_eq!(done.outcome().count, 2);
        assert!(done.outcome().now.is_none());
    }

    #[tokio::test]
    async fn skipping_zero_skips_one() {
        let (data, call, ids, _) = queue_of(3).await;
        let g = guard(&data).await;
        let done = skip_on(&g, &call, 0, None).await.unwrap();
        assert_eq!(done.outcome().count, 1);
        assert_eq!(done.outcome().now.as_ref().map(|t| t.id), Some(ids[1]));
        assert_eq!(ids_of(&call).await, ids[1..].to_vec());
    }

    #[tokio::test]
    async fn a_stale_expect_skips_nothing() {
        let (data, call, ids, mut rx) = queue_of(3).await;
        let g = guard(&data).await;
        let got = skip_on(&g, &call, 1, Some(ids[1])).await;
        assert!(matches!(got, Err(OpRefused::Stale)));
        assert_eq!(ids_of(&call).await, ids);
        assert!(recorded(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn the_current_expect_skips() {
        let (data, call, ids, _) = queue_of(2).await;
        let g = guard(&data).await;
        let _ = skip_on(&g, &call, 1, Some(ids[0])).await.unwrap();
        assert_eq!(ids_of(&call).await, vec![ids[1]]);
    }

    #[tokio::test]
    async fn skipping_an_empty_queue_is_nothing_playing() {
        let (data, call, _, _) = queue_of(0).await;
        let g = guard(&data).await;
        assert!(matches!(
            skip_on(&g, &call, 1, None).await,
            Err(OpRefused::NothingPlaying)
        ));
    }

    #[test]
    fn the_threshold_is_half_the_channel() {
        assert_eq!(skip_threshold(4), 2);
        assert_eq!(skip_threshold(3), 1);
        assert_eq!(skip_threshold(1), 0);
    }

    #[test]
    fn the_message_names_what_plays_next_or_says_skipped() {
        use crate::music::remote::TrackSummary;
        let next = TrackSummary {
            id: uuid::Uuid::nil(),
            title: Some("x".into()),
            url: Some("https://e/x".into()),
            duration: None,
            requester: None,
        };
        assert!(matches!(
            Skipped {
                now: Some(next),
                count: 1
            }
            .message(),
            CrackedMessage::SkipTo { .. }
        ));
        assert!(matches!(
            Skipped {
                now: None,
                count: 1
            }
            .message(),
            CrackedMessage::Skip
        ));
        assert!(matches!(
            Skipped {
                now: None,
                count: 2
            }
            .message(),
            CrackedMessage::SkipAll
        ));
    }

    #[tokio::test]
    async fn votes_count_distinct_voters_against_the_threshold() {
        let data = crate::Data(Arc::new(crate::DataInner::default()));
        let (a, b) = (UserId::new(5), UserId::new(6));
        assert_eq!(cast_vote(&data, GUILD, a, 2).await, Err(1));
        assert_eq!(cast_vote(&data, GUILD, a, 2).await, Err(1));
        assert_eq!(cast_vote(&data, GUILD, b, 2).await, Ok(()));
        assert_eq!(cast_vote(&data, GuildId::new(2), a, 0).await, Ok(()));
    }
}
