//! Queue edits: remove, shuffle, move, clear. Each changes what the queue
//! messages show, so each settles `QueueMessages`.
use super::*;
use crate::{
    messaging::{
        message::CrackedMessage,
        messages::{QUEUE_NO_TITLE, REMOVED_QUEUE},
    },
    music::{
        queue::{clear_from, remove_at, shuffle_behind_current},
        remote::{summary_of, MoveRefused, TrackSummary},
    },
    utils::get_track_handle_metadata,
};
use serenity::all::CreateEmbed;

/// What a remove took out: the first track (and its thumbnail, for the
/// embed) and how many went. No songbird types: crack-web reads this.
#[derive(Debug)]
pub struct Removed {
    pub first: TrackSummary,
    pub thumbnail: Option<String>,
    pub count: usize,
}

#[derive(Debug)]
pub struct Shuffled {
    pub count: usize,
}

#[derive(Debug)]
pub struct Moved {
    pub to: usize,
}

#[derive(Debug)]
pub struct Cleared {
    pub removed: usize,
}

impl Shuffled {
    pub fn message(&self) -> CrackedMessage {
        CrackedMessage::Shuffle
    }
}

impl Cleared {
    pub fn message(&self) -> CrackedMessage {
        CrackedMessage::Clear
    }
}

/// The single-track `/remove` embed; never panics on missing metadata.
pub fn removed_embed(first: &TrackSummary, thumbnail: Option<&str>) -> CreateEmbed<'static> {
    let title = first.title.as_deref().unwrap_or(QUEUE_NO_TITLE);
    let value = match &first.url {
        Some(url) => format!("[**{title}**]({url})"),
        None => format!("**{title}**"),
    };
    let embed = CreateEmbed::default().field(REMOVED_QUEUE, value, false);
    match thumbnail {
        Some(t) => embed.thumbnail(t.to_owned(), None),
        None => embed,
    }
}

/// `Index(n)` is queue position `n`, `Range(a, b)` is `a..=b` (both 1-based
/// as `/remove` takes them, 0 being the playing track), `Id` is by track id.
pub async fn remove(cx: &OpCx, target: Target) -> Result<Done<Removed>, OpRefused> {
    let (g, call) = begin(cx).await?;
    remove_on(&g, &call, target).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
pub(crate) async fn remove_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    target: Target,
) -> Result<Done<Removed>, OpRefused> {
    let (first, count) = {
        let handler = call.lock().await;
        let queue = handler.queue().current_queue();
        let len = queue.len();
        let (start, until) = match target {
            Target::Id(id) => {
                let pos = queue
                    .iter()
                    .position(|t| t.uuid() == id)
                    .ok_or(OpRefused::Absent)?;
                if pos == 0 {
                    return Err(OpRefused::NowPlaying);
                }
                (pos, pos)
            },
            Target::Index(start) => (start, start),
            Target::Range(start, end) => (start, end),
        };
        if len <= 1 {
            return Err(OpRefused::QueueEmpty);
        }
        let until = until.min(len - 1);
        if start >= len {
            return Err(OpRefused::OutOfRange {
                what: "index",
                got: start,
                min: 1,
                max: len,
            });
        }
        if until < start {
            return Err(OpRefused::OutOfRange {
                what: "until",
                got: until,
                min: start,
                max: len,
            });
        }
        let first = queue[start].clone();
        // Removing repeatedly at `start` shifts each later track down into
        // it, so this reaches the same tracks as a drain of the range.
        for _ in start..=until {
            remove_at(g, &handler, start);
        }
        (first, until - start + 1)
    };
    // 🔑 The Call lock is released: reading metadata awaits. Removing a
    // track does not drop its metadata. Nothing below may fail: the removal
    // is done and recorded, so it is reported, metadata or not.
    let meta = get_track_handle_metadata(&first).await.unwrap_or_default();
    let thumbnail = meta.thumbnail.clone();
    let first = summary_of(&first, meta).await;
    Ok(Done {
        outcome: Removed {
            first,
            thumbnail,
            count,
        },
        settle: Settle::QueueMessages,
        call: Some(call.clone()),
    })
}

/// Shuffle what is queued behind the playing track. A short queue is not
/// refused (`/shuffle` never refused one): `count` is then 0.
pub async fn shuffle(cx: &OpCx) -> Result<Done<Shuffled>, OpRefused> {
    let (g, call) = begin(cx).await?;
    shuffle_on(&g, &call).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
pub(crate) async fn shuffle_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
) -> Result<Done<Shuffled>, OpRefused> {
    let handler = call.lock().await;
    let upcoming = handler.queue().len().saturating_sub(1);
    shuffle_behind_current(g, &handler);
    Ok(Done {
        outcome: Shuffled {
            count: if upcoming >= 2 { upcoming } else { 0 },
        },
        settle: Settle::QueueMessages,
        call: Some(call.clone()),
    })
}

/// `Index(at)`: 1-based queue positions as `/movesong` takes them; `to`
/// likewise. `Id(id)`: `to` is 0-based among upcoming tracks, clamped (the
/// dashboard's move). `Moved.to` is the position used.
pub async fn move_track(cx: &OpCx, target: Target, to: usize) -> Result<Done<Moved>, OpRefused> {
    let (g, call) = begin(cx).await?;
    move_on(&g, &call, target, to).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
pub(crate) async fn move_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    target: Target,
    to: usize,
) -> Result<Done<Moved>, OpRefused> {
    let handler = call.lock().await;
    let used = match target {
        Target::Index(at) => {
            let len = handler.queue().len();
            if !(at > 0 && at < len) {
                return Err(OpRefused::Invalid("Index for `at` out of bounds"));
            }
            if !(to > 0 && to < len) {
                return Err(OpRefused::Invalid("Index for `to` out of bounds"));
            }
            crate::music::queue::move_track(g, &handler, at, to);
            to
        },
        Target::Id(id) => {
            crate::music::queue::move_track_by_id(g, &handler, id, to).map_err(|e| match e {
                MoveRefused::Absent => OpRefused::Absent,
                MoveRefused::NowPlaying => OpRefused::NowPlaying,
                MoveRefused::NotPlaying => OpRefused::NotConnected,
                MoveRefused::GameInProgress => OpRefused::GameInProgress,
            })?
        },
        Target::Range(..) => return Err(OpRefused::Invalid("A move takes one track")),
    };
    Ok(Done {
        outcome: Moved { to: used },
        settle: Settle::QueueMessages,
        call: Some(call.clone()),
    })
}

/// Drop everything behind the playing track.
pub async fn clear(cx: &OpCx) -> Result<Done<Cleared>, OpRefused> {
    let (g, call) = begin(cx).await?;
    clear_on(&g, &call).await
}

#[expect(
    clippy::disallowed_methods,
    reason = "music::ops is where these are orchestrated"
)]
pub(crate) async fn clear_on(
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
) -> Result<Done<Cleared>, OpRefused> {
    let handler = call.lock().await;
    let len = handler.queue().len();
    if len <= 1 {
        return Err(OpRefused::QueueEmpty);
    }
    clear_from(g, &handler, 1);
    Ok(Done {
        outcome: Cleared { removed: len - 1 },
        settle: Settle::QueueMessages,
        call: Some(call.clone()),
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::music::{audit::Action, ops::test_support::*};

    #[tokio::test]
    async fn remove_by_index_and_range() {
        let (data, call, ids, mut rx) = queue_of(5).await;
        let g = guard(&data).await;
        let d = remove_on(&g, &call, Target::Index(2)).await.unwrap();
        assert_eq!(
            (d.outcome().first.id, d.outcome().count, d.settle().clone()),
            (ids[2], 1, Settle::QueueMessages)
        );
        assert_eq!(d.outcome().first.title.as_deref(), Some("t2"));
        assert_eq!(ids_of(&call).await, vec![ids[0], ids[1], ids[3], ids[4]]);
        let _ = remove_on(&g, &call, Target::Range(1, 2)).await.unwrap();
        assert_eq!(ids_of(&call).await, vec![ids[0], ids[4]]);
        assert_eq!(recorded(&mut rx).len(), 3);
    }

    #[tokio::test]
    async fn remove_by_id_refuses_the_playing_track_and_unknown_ids() {
        let (data, call, ids, _) = queue_of(3).await;
        let g = guard(&data).await;
        assert!(matches!(
            remove_on(&g, &call, Target::Id(ids[0])).await,
            Err(OpRefused::NowPlaying)
        ));
        assert!(matches!(
            remove_on(&g, &call, Target::Id(uuid::Uuid::from_u128(7))).await,
            Err(OpRefused::Absent)
        ));
        let _ = remove_on(&g, &call, Target::Id(ids[2])).await.unwrap();
        assert_eq!(ids_of(&call).await, vec![ids[0], ids[1]]);
    }

    #[tokio::test]
    async fn remove_keeps_its_old_refusals() {
        let (data, call, _, _) = queue_of(1).await;
        let g = guard(&data).await;
        assert!(matches!(
            remove_on(&g, &call, Target::Index(1)).await,
            Err(OpRefused::QueueEmpty)
        ));
        let (data, call, _, _) = queue_of(3).await;
        let g = guard(&data).await;
        assert!(matches!(
            remove_on(&g, &call, Target::Index(3)).await,
            Err(OpRefused::OutOfRange {
                what: "index",
                got: 3,
                min: 1,
                max: 3
            })
        ));
    }

    #[test]
    fn the_removed_embed_survives_missing_metadata() {
        let blank = TrackSummary {
            id: uuid::Uuid::nil(),
            title: None,
            url: None,
            duration: None,
            requester: None,
        };
        let _ = removed_embed(&blank, None);
    }

    #[tokio::test]
    async fn shuffle_keeps_the_playing_track_and_the_set() {
        let (data, call, ids, mut rx) = queue_of(6).await;
        let g = guard(&data).await;
        let d = shuffle_on(&g, &call).await.unwrap();
        let after = ids_of(&call).await;
        assert_eq!(after[0], ids[0]);
        let mut sorted = after.clone();
        sorted.sort();
        let mut want = ids.clone();
        want.sort();
        assert_eq!(sorted, want);
        assert_eq!(
            (d.outcome().count, d.settle().clone()),
            (5, Settle::QueueMessages)
        );
        assert!(matches!(
            recorded(&mut rx).as_slice(),
            [Action::Shuffle { count: 5 }]
        ));
    }

    #[tokio::test]
    async fn move_by_index_and_by_id() {
        let (data, call, ids, _) = queue_of(4).await;
        let g = guard(&data).await;
        let d = move_on(&g, &call, Target::Index(3), 1).await.unwrap();
        assert_eq!(
            (d.outcome().to, d.settle().clone()),
            (1, Settle::QueueMessages)
        );
        assert_eq!(ids_of(&call).await, vec![ids[0], ids[3], ids[1], ids[2]]);
        let d = move_on(&g, &call, Target::Id(ids[1]), 99).await.unwrap();
        assert_eq!(d.outcome().to, 2);
        assert_eq!(ids_of(&call).await, vec![ids[0], ids[3], ids[2], ids[1]]);
        assert!(matches!(
            move_on(&g, &call, Target::Index(0), 1).await,
            Err(OpRefused::Invalid("Index for `at` out of bounds"))
        ));
        assert!(matches!(
            move_on(&g, &call, Target::Id(ids[0]), 1).await,
            Err(OpRefused::NowPlaying)
        ));
        assert!(matches!(
            move_on(&g, &call, Target::Id(uuid::Uuid::from_u128(7)), 1).await,
            Err(OpRefused::Absent)
        ));
    }

    #[tokio::test]
    async fn clear_keeps_only_the_playing_track() {
        let (data, call, ids, _) = queue_of(4).await;
        let g = guard(&data).await;
        let d = clear_on(&g, &call).await.unwrap();
        assert_eq!(d.outcome().removed, 3);
        assert_eq!(ids_of(&call).await, vec![ids[0]]);
        let (data, call, _, _) = queue_of(1).await;
        let g = guard(&data).await;
        assert!(matches!(
            clear_on(&g, &call).await,
            Err(OpRefused::QueueEmpty)
        ));
    }
}
