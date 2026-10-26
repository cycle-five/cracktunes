//! Add one track the member chose (a dashboard search hit) to the end of the
//! queue or to play next. The one op that queues; it closes the gap the ops
//! layer left. Spec §2.
use super::*;
use crate::{
    db::worker_pool::MetadataMsg,
    music::queue::{enqueue_resolved_track_next, enqueue_resolved_tracks_back},
};
use crack_common::messages::ADD_UNPLAYABLE;
use crack_testing::ResolvedTrack;
use crack_types::SavedTrack;
use serenity::all::{ChannelId, GenericChannelId, UserId};

/// Where an added track goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    End,
    Next,
}

/// What an add queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub title: Option<String>,
}

/// Add `track` for the acting member. `member_name` is written to the play
/// log with it: `User::insert_or_update_user` overwrites the stored name with
/// whatever it is given, so it must be the member's real one.
pub async fn add(
    cx: &OpCx,
    track: SavedTrack,
    at: At,
    member_name: &str,
) -> Result<Done<Added>, OpRefused> {
    let user = cx.actor.user().ok_or(OpRefused::Invalid(ADD_UNPLAYABLE))?;
    // History hits come from play_log rows that may predate the guard /play
    // applies: a re-add must not hand a LAN link to yt-dlp's generic extractor.
    let public = match url::Url::parse(&track.url) {
        Ok(u) => crack_types::is_public_http_url(&u).await,
        Err(_) => false,
    };
    if !public {
        return Err(OpRefused::Invalid(ADD_UNPLAYABLE));
    }
    let (g, call) = begin(cx).await?;
    add_with(cx, &g, &call, &track, at, user, member_name).await
}

/// [`add`] past the lease and the call lookup, so a test can run all of it on
/// an offline call.
async fn add_with(
    cx: &OpCx,
    g: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    track: &SavedTrack,
    at: At,
    user: UserId,
    member_name: &str,
) -> Result<Done<Added>, OpRefused> {
    // A member queued something: autoplay's buffered picks no longer apply,
    // as for /play (Ruling 49).
    cx.data.autoplay_buffer.clear(cx.guild_id);
    let added = add_on(g, call, track, at, user).await?;
    let channel = call
        .lock()
        .await
        .current_channel()
        .map(|c| ChannelId::new(c.get()));
    if let Some(tx) = &cx.data.db_channel {
        let msg = play_log_message(track, user, member_name, cx.guild_id, channel);
        // try_send, as the /play paths do: a wedged writer must not block an add.
        if let Err(e) = tx.try_send(msg) {
            tracing::error!("add: could not queue the play log write: {e}");
        }
    }
    Ok(Done {
        outcome: added,
        settle: Settle::QueueMessages,
        call: Some(call.clone()),
    })
}

/// The add itself, under `guard`, on `call`. No Discord, no database.
pub(crate) async fn add_on(
    guard: &QueueGuard,
    call: &Arc<Mutex<Call>>,
    track: &SavedTrack,
    at: At,
    user: UserId,
) -> Result<Added, OpRefused> {
    let resolved = ResolvedTrack::from_saved(track).with_user_id(user);
    let http = crate::http_utils::get_client_old().clone();
    match at {
        At::End => {
            let inserted = enqueue_resolved_tracks_back(guard, call, vec![resolved], http)
                .await
                .map_err(unplayable)?;
            if inserted.count() == 0 {
                return Err(OpRefused::Invalid(ADD_UNPLAYABLE));
            }
        },
        At::Next => {
            enqueue_resolved_track_next(guard, call, resolved, http)
                .await
                .map_err(unplayable)?;
        },
    }
    Ok(Added {
        title: track.title.clone(),
    })
}

/// An enqueue error as the refusal the member sees; the cause goes to the log.
fn unplayable(e: CrackedError) -> OpRefused {
    tracing::warn!("add: could not enqueue the track: {e}");
    OpRefused::Invalid(ADD_UNPLAYABLE)
}

/// The play-log write for an add: the track as metadata, the member, and the
/// bot's voice channel (the web has no text channel to name).
pub(crate) fn play_log_message(
    track: &SavedTrack,
    user: UserId,
    member_name: &str,
    guild: GuildId,
    voice: Option<ChannelId>,
) -> MetadataMsg {
    MetadataMsg {
        aux_metadata: track.to_metadata(),
        user_id: Some(user),
        username: Some(member_name.to_owned()),
        guild_id: guild,
        channel_id: GenericChannelId::new(voice.map_or(1, ChannelId::get)),
    }
}

#[cfg(test)]
mod test {
    use super::super::test_support::{guard, ids_of, queue_of, recorded, GUILD};
    use super::*;
    use crate::music::audit::{Action, AddAt};
    use serenity::all::UserId;

    fn song(n: u32) -> SavedTrack {
        SavedTrack {
            url: format!("https://www.youtube.com/watch?v=song{n}"),
            title: Some(format!("Song {n}")),
            artist: Some("Band".into()),
            duration: Some(std::time::Duration::from_secs(100)),
        }
    }

    async fn requester_of(call: &Arc<Mutex<Call>>, at: usize) -> UserId {
        let h = call.lock().await.queue().current_queue()[at].clone();
        crate::music::track_data::get_requesting_user(&h)
            .await
            .unwrap()
    }

    async fn url_of(call: &Arc<Mutex<Call>>, at: usize) -> Option<String> {
        let h = call.lock().await.queue().current_queue()[at].clone();
        crate::music::track_data::get_track_handle_metadata(&h)
            .await
            .ok()?
            .source_url
    }

    #[tokio::test]
    async fn end_appends_as_the_member_and_records_one_back_add() {
        let (data, call, ids, mut rx) = queue_of(3).await;
        let g = guard(&data).await;
        let added = add_on(&g, &call, &song(7), At::End, UserId::new(42))
            .await
            .unwrap();
        assert_eq!(
            added,
            Added {
                title: Some("Song 7".into())
            }
        );
        let now = ids_of(&call).await;
        assert_eq!((now.len(), &now[..3]), (4, &ids[..]));
        assert_eq!(
            url_of(&call, 3).await.as_deref(),
            Some("https://www.youtube.com/watch?v=song7")
        );
        assert_eq!(requester_of(&call, 3).await, UserId::new(42));
        assert!(
            matches!(&recorded(&mut rx)[..], [Action::Add { at: AddAt::Back, tracks }] if tracks.len() == 1)
        );
    }

    #[tokio::test]
    async fn next_lands_right_behind_the_playing_track() {
        let (data, call, ids, mut rx) = queue_of(3).await;
        let g = guard(&data).await;
        add_on(&g, &call, &song(8), At::Next, UserId::new(42))
            .await
            .unwrap();
        let now = ids_of(&call).await;
        assert_eq!((now[0], now[2], now[3]), (ids[0], ids[1], ids[2]));
        assert_eq!(
            url_of(&call, 1).await.as_deref(),
            Some("https://www.youtube.com/watch?v=song8")
        );
        assert!(matches!(
            &recorded(&mut rx)[..],
            [Action::Add {
                at: AddAt::Front,
                ..
            }]
        ));
    }

    #[tokio::test]
    async fn next_with_nothing_between_stays_last() {
        for n in [0, 1] {
            let (data, call, ids, _rx) = queue_of(n).await;
            let g = guard(&data).await;
            add_on(&g, &call, &song(9), At::Next, UserId::new(42))
                .await
                .unwrap();
            let now = ids_of(&call).await;
            assert_eq!(
                (now.len(), &now[..n]),
                (n + 1, &ids[..]),
                "{n} queued before"
            );
            assert_eq!(
                url_of(&call, n).await.as_deref(),
                Some("https://www.youtube.com/watch?v=song9")
            );
        }
    }

    #[tokio::test]
    async fn add_refuses_without_a_call() {
        let cx = super::super::test_support::cx_without_call();
        assert!(matches!(
            add(&cx, song(1), At::End, "m").await,
            Err(OpRefused::NotConnected)
        ));
    }

    /// Ruling 6: a LAN or loopback link is refused before any lease or
    /// enqueue (`Invalid`, not `NotConnected`). IP literals, so no DNS.
    #[tokio::test]
    async fn add_refuses_a_non_public_url_before_the_lease() {
        let cx = super::super::test_support::cx_without_call();
        for url in [
            "http://127.0.0.1/x",
            "http://192.168.1.5/x",
            "http://[::1]/x",
            "file:///etc/passwd",
            "not a url",
        ] {
            let track = SavedTrack {
                url: url.into(),
                ..song(1)
            };
            assert!(
                matches!(
                    add(&cx, track, At::End, "m").await,
                    Err(OpRefused::Invalid(_))
                ),
                "{url}"
            );
        }
    }

    #[tokio::test]
    async fn add_refuses_during_a_game() {
        let d = super::super::test_support::test_music_data();
        d.claim_playback(GUILD, crate::music::PlaybackOwner::Game)
            .unwrap();
        let cx = OpCx {
            data: d,
            ..super::super::test_support::cx_without_call()
        };
        assert!(matches!(
            add(&cx, song(1), At::End, "m").await,
            Err(OpRefused::GameInProgress)
        ));
    }

    #[test]
    fn the_play_log_message_names_the_member_and_the_track() {
        let msg = play_log_message(
            &song(3),
            UserId::new(42),
            "lothrop",
            GUILD,
            Some(serenity::all::ChannelId::new(77)),
        );
        assert_eq!(msg.user_id, Some(UserId::new(42)));
        assert_eq!(msg.username.as_deref(), Some("lothrop"));
        assert_eq!(msg.guild_id, GUILD);
        assert_eq!(msg.channel_id.get(), 77);
        assert_eq!(
            msg.aux_metadata.source_url.as_deref(),
            Some("https://www.youtube.com/watch?v=song3")
        );
        assert_eq!(msg.aux_metadata.title.as_deref(), Some("Song 3"));
    }

    /// Ruling 49 and the play log, through the whole op past the lease lookup.
    #[tokio::test]
    async fn an_add_clears_the_autoplay_buffer_and_writes_the_play_log() {
        use crack_musicreco::{Playable, Recommendation};
        let (tx, mut log) = tokio::sync::mpsc::channel(4);
        let (data, call, _ids, _rx) = queue_of(1).await;
        let data = MusicData {
            db_channel: Some(tx),
            ..data
        };
        let pick = Recommendation {
            artist: "A".into(),
            title: "stale".into(),
            playable: Playable::SearchQuery("A - stale".into()),
            isrc: None,
            source: "test".into(),
        };
        // One pick is served and the other stays buffered.
        let buffered = vec![pick.clone(), pick];
        assert!(data
            .autoplay_buffer
            .next(GUILD, || async { buffered })
            .await
            .is_some());
        let g = guard(&data).await;
        let cx = OpCx {
            data: data.clone(),
            ..super::super::test_support::cx_without_call()
        };
        let done = add_with(
            &cx,
            &g,
            &call,
            &song(1),
            At::End,
            UserId::new(42),
            "lothrop",
        )
        .await
        .unwrap();
        assert_eq!(done.outcome().title.as_deref(), Some("Song 1"));
        let left = cx
            .data
            .autoplay_buffer
            .next(GUILD, || async { Vec::new() })
            .await;
        assert!(left.is_none(), "the buffered pick survived an add");
        let msg = log.try_recv().expect("one play-log write");
        assert_eq!(msg.username.as_deref(), Some("lothrop"));
        assert_eq!(msg.user_id, Some(UserId::new(42)));
        assert!(log.try_recv().is_err(), "exactly one write");
    }
}
