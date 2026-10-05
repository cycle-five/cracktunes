//! The dashboard's wire types -- typed serde, one enum per direction -- and
//! the conversion from crack-core's `QueueState`.

use crack_core::guild::plan::Plan;
use crack_core::music::remote::{Control, QueueState, Requester, TrackSummary};
use serde::{Deserialize, Serialize};
use serenity::all::UserId;
use std::hash::{DefaultHasher, Hash, Hasher};
use uuid::Uuid;

/// `rev` is masked to 53 bits: JavaScript numbers are exact up to 2^53, and
/// the browser echoes `rev` back when it compares views.
pub const REV_MASK: u64 = (1 << 53) - 1;

/// One track as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackView {
    pub id: Uuid,
    pub title: String,
    pub url: Option<String>,
    pub duration_secs: Option<u64>,
    pub requester: Option<String>,
}

/// A guild's queue as the page shows it. Sent whole, never as a diff: a
/// client that missed an event is right again after the next one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum QueueView {
    Idle,
    Hidden,
    Playing {
        now: TrackView,
        upcoming: Vec<TrackView>,
        rev: u64,
        paused: bool,
        looping: bool,
    },
}

/// What the page renders: the shared view plus this viewer's permission.
/// Inlined into the page and sent as every SSE event.
#[derive(Debug, Serialize)]
pub struct PageState<'a> {
    pub view: &'a QueueView,
    pub can_control: bool,
    pub plan: PlanView,
}

/// A server's plan as the page knows it: premium unlocks the controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanView {
    Free,
    Premium,
}

impl From<Plan> for PlanView {
    fn from(p: Plan) -> Self {
        match p {
            Plan::Free => PlanView::Free,
            Plan::Premium => PlanView::Premium,
        }
    }
}

/// `POST /g/{id}/control`: one control. `Skip` names the track the member saw
/// playing, so a skip that lands after the song changed is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlRequest {
    Skip { id: Uuid },
    Pause,
    Resume,
    Repeat { on: bool },
    Remove { id: Uuid },
    Shuffle,
}

impl From<ControlRequest> for Control {
    fn from(r: ControlRequest) -> Self {
        match r {
            ControlRequest::Skip { id } => Control::Skip { expect: id },
            ControlRequest::Pause => Control::Pause,
            ControlRequest::Resume => Control::Resume,
            ControlRequest::Repeat { on } => Control::Repeat { on },
            ControlRequest::Remove { id } => Control::Remove { id },
            ControlRequest::Shuffle => Control::Shuffle,
        }
    }
}

/// The answer to a control. `Done` and `Conflict` carry the view to render.
#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ControlResult {
    Done { view: QueueView },
    Conflict { view: QueueView },
    NotAllowed,
    PremiumRequired,
    GameInProgress,
    NotPlaying,
    Failed,
    TooMany,
}

/// `POST /g/{id}/move`: move track `id` to position `to` among the upcoming
/// tracks (0-based).
#[derive(Debug, Deserialize)]
pub struct MoveRequest {
    pub id: Uuid,
    pub to: usize,
}

/// The answer to a move. `Moved` and `Conflict` carry the view to render.
#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MoveResult {
    Moved { view: QueueView },
    Conflict { view: QueueView },
    NotAllowed,
    GameInProgress,
    NotPlaying,
}

/// A change token for the queue's order: the ids, in order, hashed.
pub fn rev_of(tracks: &[TrackSummary]) -> u64 {
    let mut h = DefaultHasher::new();
    for t in tracks {
        t.id.hash(&mut h);
    }
    h.finish() & REV_MASK
}

/// Only http(s) links reach the page: the url is yt-dlp metadata, and an
/// `href` accepts any scheme (`javascript:`, `data:`).
fn is_web_url(u: &str) -> bool {
    let lower = u.get(..8).unwrap_or(u).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn track_view(t: TrackSummary, name_of: &impl Fn(UserId) -> Option<String>) -> TrackView {
    TrackView {
        id: t.id,
        title: t.title.unwrap_or_else(|| "Unknown title".to_owned()),
        url: t.url.filter(|u| is_web_url(u)),
        duration_secs: t.duration.map(|d| d.as_secs()),
        requester: match t.requester {
            Some(Requester::Auto) => Some("(auto)".to_owned()),
            Some(Requester::User(u)) => name_of(u),
            None => None,
        },
    }
}

/// Build the page's view of a queue. `name_of` names a requester, or `None`.
pub fn view_from_state(state: QueueState, name_of: impl Fn(UserId) -> Option<String>) -> QueueView {
    match state {
        QueueState::Idle => QueueView::Idle,
        QueueState::Hidden => QueueView::Hidden,
        QueueState::Playing {
            tracks,
            paused,
            looping,
            ..
        } => {
            let rev = rev_of(&tracks);
            let mut tracks = tracks.into_iter().map(|t| track_view(t, &name_of));
            match tracks.next() {
                None => QueueView::Idle,
                Some(now) => QueueView::Playing {
                    now,
                    upcoming: tracks.collect(),
                    rev,
                    paused,
                    looping,
                },
            }
        },
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crack_core::music::remote::{QueueState, Requester, TrackSummary};
    use serenity::all::{ChannelId, UserId};
    use std::time::Duration;

    fn t(n: u128, title: Option<&str>, requester: Option<Requester>) -> TrackSummary {
        TrackSummary {
            id: Uuid::from_u128(n),
            title: title.map(str::to_owned),
            url: Some(format!("https://example.com/{n}")),
            duration: Some(Duration::from_secs(61)),
            requester,
        }
    }

    #[test]
    fn only_http_urls_reach_the_page() {
        let view = |url: &str| {
            let mut track = t(1, Some("x"), None);
            track.url = Some(url.to_owned());
            track_view(track, &names).url
        };
        assert_eq!(view("javascript:alert(1)"), None);
        assert_eq!(view("data:text/html,x"), None);
        assert_eq!(view(""), None);
        assert_eq!(
            view("HTTPS://example.com/x").as_deref(),
            Some("HTTPS://example.com/x")
        );
        assert_eq!(
            view("http://example.com").as_deref(),
            Some("http://example.com")
        );
    }

    fn playing(tracks: Vec<TrackSummary>) -> QueueState {
        QueueState::Playing {
            bot_channel: ChannelId::new(9),
            tracks,
            paused: false,
            looping: false,
        }
    }

    fn names(u: UserId) -> Option<String> {
        (u == UserId::new(7)).then(|| "Seven".to_owned())
    }

    #[test]
    fn the_first_track_is_now_and_the_rest_are_upcoming() {
        let v = view_from_state(
            playing(vec![
                t(1, Some("A"), Some(Requester::User(UserId::new(7)))),
                t(2, None, Some(Requester::Auto)),
                t(3, Some("C"), Some(Requester::User(UserId::new(8)))),
            ]),
            names,
        );
        let QueueView::Playing { now, upcoming, .. } = v else {
            panic!("playing")
        };
        assert_eq!(now.title, "A");
        assert_eq!(now.requester.as_deref(), Some("Seven"));
        assert_eq!(now.duration_secs, Some(61));
        assert_eq!(upcoming.len(), 2);
        assert_eq!(upcoming[0].title, "Unknown title");
        assert_eq!(upcoming[0].requester.as_deref(), Some("(auto)"));
        assert_eq!(upcoming[1].requester, None, "an uncached user has no name");
    }

    #[test]
    fn rev_follows_the_order_and_nothing_else() {
        let a = rev_of(&[t(1, Some("A"), None), t(2, Some("B"), None)]);
        let same_ids_new_titles = rev_of(&[t(1, Some("x"), None), t(2, Some("y"), None)]);
        let swapped = rev_of(&[t(2, Some("B"), None), t(1, Some("A"), None)]);
        assert_eq!(a, same_ids_new_titles);
        assert_ne!(a, swapped);
    }

    #[test]
    fn rev_fits_in_a_javascript_number() {
        for n in 0..200u128 {
            assert!(rev_of(&[t(n, None, None)]) <= REV_MASK);
        }
    }

    #[test]
    fn idle_hidden_and_an_empty_playing_state_map_across() {
        assert_eq!(view_from_state(QueueState::Idle, names), QueueView::Idle);
        assert_eq!(
            view_from_state(QueueState::Hidden, names),
            QueueView::Hidden
        );
        assert_eq!(view_from_state(playing(vec![]), names), QueueView::Idle);
    }

    #[test]
    fn the_wire_format_is_tagged_snake_case() {
        #[derive(serde::Deserialize)]
        struct Tagged {
            state: String,
        }
        let hidden: Tagged =
            serde_json::from_str(&serde_json::to_string(&QueueView::Hidden).unwrap()).unwrap();
        assert_eq!(hidden.state, "hidden");

        #[derive(serde::Deserialize)]
        struct Res {
            result: String,
        }
        let r: Res =
            serde_json::from_str(&serde_json::to_string(&MoveResult::NotAllowed).unwrap()).unwrap();
        assert_eq!(r.result, "not_allowed");

        let req: MoveRequest =
            serde_json::from_str(r#"{"id":"00000000-0000-0000-0000-000000000005","to":3}"#)
                .unwrap();
        assert_eq!((req.id, req.to), (Uuid::from_u128(5), 3));
    }

    #[test]
    fn the_view_carries_the_flags_through_and_serializes_them() {
        let v = view_from_state(
            QueueState::Playing {
                bot_channel: ChannelId::new(9),
                tracks: vec![t(1, Some("A"), None)],
                paused: true,
                looping: false,
            },
            names,
        );
        let QueueView::Playing {
            paused, looping, ..
        } = &v
        else {
            panic!("not playing: {v:?}")
        };
        assert_eq!((*paused, *looping), (true, false));
        let json = serde_json::to_string(&v).unwrap();
        assert!(
            json.contains("\"paused\":true") && json.contains("\"looping\":false"),
            "{json}"
        );
    }

    #[test]
    fn control_requests_parse_by_type() {
        let p = |s: &str| serde_json::from_str::<ControlRequest>(s);
        assert_eq!(
            p(r#"{"type":"skip","id":"00000000-0000-0000-0000-000000000005"}"#).unwrap(),
            ControlRequest::Skip {
                id: Uuid::from_u128(5)
            }
        );
        assert_eq!(p(r#"{"type":"pause"}"#).unwrap(), ControlRequest::Pause);
        assert_eq!(
            p(r#"{"type":"repeat","on":true}"#).unwrap(),
            ControlRequest::Repeat { on: true }
        );
        assert!(p(r#"{"type":"stop"}"#).is_err(), "not a dashboard control");
        assert!(
            p(r#"{"type":"skip"}"#).is_err(),
            "skip needs the id it saw playing"
        );
        assert!(
            p(r#"{"type":"repeat"}"#).is_err(),
            "repeat is explicit, not a toggle"
        );
    }

    #[test]
    fn answers_and_plan_serialize_tagged() {
        let s = |r: &ControlResult| serde_json::to_string(r).unwrap();
        assert_eq!(
            s(&ControlResult::PremiumRequired),
            r#"{"result":"premium_required"}"#
        );
        assert_eq!(s(&ControlResult::TooMany), r#"{"result":"too_many"}"#);
        let st = PageState {
            view: &QueueView::Idle,
            can_control: false,
            plan: PlanView::Free,
        };
        assert!(serde_json::to_string(&st)
            .unwrap()
            .contains(r#""plan":"free""#));
    }

    #[test]
    fn requests_become_core_controls() {
        use crack_core::music::remote::Control;
        assert_eq!(
            Control::from(ControlRequest::Remove {
                id: Uuid::from_u128(3)
            }),
            Control::Remove {
                id: Uuid::from_u128(3)
            }
        );
        assert_eq!(
            Control::from(ControlRequest::Skip {
                id: Uuid::from_u128(1)
            }),
            Control::Skip {
                expect: Uuid::from_u128(1)
            }
        );
    }
}
