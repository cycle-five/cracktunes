//! Search and add: `GET /g/{g}/search` and `POST /g/{g}/add`, their wire
//! types and their gates. Spec: docs/superpowers/specs/2026-10-13-dashboard-search-and-add-design.md §3.

use crate::{
    access::{decide, Access},
    backend::Backend,
    history::BadQuery,
    hits::HitMemo,
    routes::{
        error_json, is_json, not_found, parse_guild, plan_or_free, unavailable, user_id, Session,
        WebState,
    },
    view::{PlanView, QueueView},
};
use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use crack_core::guild::plan::Plan;
use crack_core::messaging::messages::{
    QUEUE_NO_TITLE, SEARCH_CONTROL, SEARCH_NO_SOURCES, SEARCH_TOO_LONG, SEARCH_TOO_SHORT,
    SEARCH_UNKNOWN_SOURCE, SEARCH_UNREADABLE,
};
use crack_core::music::remote::{AddRefused, At};
use crack_core::music::search::{
    Hit, HitDetail, Query, QueryRefused, SavedTrack, Source, SourceResult, SourceSet,
};
use crack_model::{CommunityRef, UserRef};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SourceName {
    #[serde(rename = "queue")]
    Queue,
    #[serde(rename = "history")]
    History,
    #[serde(rename = "youtube")]
    YouTube,
    #[serde(rename = "soundcloud")]
    SoundCloud,
}

impl From<Source> for SourceName {
    fn from(s: Source) -> Self {
        match s {
            Source::Queue => Self::Queue,
            Source::History => Self::History,
            Source::YouTube => Self::YouTube,
            Source::SoundCloud => Self::SoundCloud,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusView {
    Ok,
    Hidden,
    NoDatabase,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HitDetailView {
    Queue { position: usize },
    History { plays: u32, last_unix: i64 },
    Found,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HitView {
    pub id: Uuid,
    pub title: String,
    pub artist: Option<String>,
    pub duration_secs: Option<u64>,
    pub url: String,
    pub detail: HitDetailView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceView {
    pub source: SourceName,
    pub status: StatusView,
    pub hits: Vec<HitView>,
}

/// The answer to a search.
#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub sources: Vec<SourceView>,
}

pub(crate) fn hit_view(id: Uuid, h: &Hit) -> HitView {
    HitView {
        id,
        title: h
            .track
            .title
            .clone()
            .unwrap_or_else(|| QUEUE_NO_TITLE.to_owned()),
        artist: h.track.artist.clone(),
        duration_secs: h.track.duration.map(|d| d.as_secs()),
        url: h.track.url.clone(),
        detail: match &h.detail {
            HitDetail::Queue { position } => HitDetailView::Queue {
                position: *position,
            },
            HitDetail::History { plays, last } => HitDetailView::History {
                plays: *plays,
                last_unix: last.timestamp(),
            },
            HitDetail::Found => HitDetailView::Found,
        },
    }
}

#[derive(Debug, Deserialize)]
struct RawSearch {
    q: Option<String>,
    src: Option<String>,
}

/// `q` and `src` from the query string.
pub(crate) fn parse_search(raw: Option<&str>) -> Result<(Query, SourceSet), BadQuery> {
    let raw: RawSearch =
        serde_urlencoded::from_str(raw.unwrap_or("")).map_err(|_| BadQuery(SEARCH_UNREADABLE))?;
    let q = Query::new(raw.q.as_deref().unwrap_or("")).map_err(|r| {
        BadQuery(match r {
            QueryRefused::TooShort => SEARCH_TOO_SHORT,
            QueryRefused::TooLong => SEARCH_TOO_LONG,
            QueryRefused::ControlCharacters => SEARCH_CONTROL,
        })
    })?;
    let mut set = SourceSet::empty();
    for name in raw
        .src
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
    {
        set = set.with(match name {
            "queue" => Source::Queue,
            "history" => Source::History,
            "youtube" => Source::YouTube,
            "soundcloud" => Source::SoundCloud,
            _ => return Err(BadQuery(SEARCH_UNKNOWN_SOURCE)),
        });
    }
    if set.is_empty() {
        return Err(BadQuery(SEARCH_NO_SOURCES));
    }
    Ok((q, set))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtView {
    End,
    Next,
}

impl From<AtView> for At {
    fn from(a: AtView) -> Self {
        match a {
            AtView::End => At::End,
            AtView::Next => At::Next,
        }
    }
}

/// `POST /g/{id}/add`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddRequest {
    pub hit: Uuid,
    pub at: AtView,
}

/// The answer to an add. `Added` carries the view to render.
#[derive(Debug, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum AddResult {
    Added { view: QueueView },
    Expired,
    NotAllowed,
    PremiumRequired,
    GameInProgress,
    NotPlaying,
    Failed,
    TooMany,
}

fn history_since(plan: PlanView) -> Option<chrono::DateTime<chrono::Utc>> {
    let plan = match plan {
        PlanView::Free => Plan::Free,
        PlanView::Premium => Plan::Premium,
    };
    plan.history_floor(chrono::Utc::now())
}

/// Store each source's hits in the memo and build the answer.
fn answer_search(
    memo: &HitMemo,
    user: &UserRef,
    g: &CommunityRef,
    results: Vec<(Source, SourceResult)>,
) -> SearchResult {
    // One pass: each hit gets its id and its view together, so an id can
    // never name a different hit than the view it travels with.
    let mut stored: Vec<(Uuid, SavedTrack)> = Vec::new();
    let sources = results
        .into_iter()
        .map(|(source, r)| {
            let (status, hits) = match r {
                SourceResult::Hits(h) => (
                    StatusView::Ok,
                    h.iter()
                        .map(|x| {
                            let id = Uuid::new_v4();
                            stored.push((id, x.track.clone()));
                            hit_view(id, x)
                        })
                        .collect(),
                ),
                SourceResult::Hidden => (StatusView::Hidden, Vec::new()),
                SourceResult::NoDatabase => (StatusView::NoDatabase, Vec::new()),
                SourceResult::Failed => (StatusView::Failed, Vec::new()),
                SourceResult::TimedOut => (StatusView::TimedOut, Vec::new()),
            };
            SourceView {
                source: source.into(),
                status,
                hits,
            }
        })
        .collect();
    memo.store(user, g, stored, Instant::now());
    SearchResult { sources }
}

/// Session, membership, query, the search limiter, the plan's window.
pub(crate) async fn search<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    match decide(&s.backend.presence(g.clone(), user.clone()).await) {
        Access::Hidden => return StatusCode::NOT_FOUND.into_response(),
        Access::Unavailable => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Access::View | Access::Control => {},
    }
    let (q, sources) = match parse_search(query.as_deref()) {
        Ok(p) => p,
        Err(bad) => return error_json(StatusCode::BAD_REQUEST, bad.0),
    };
    if !s.search_limiter.allow(&user, Instant::now()) {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let since = history_since(plan_or_free(s.backend.as_ref(), g.clone()).await);
    let results = s.backend.search(g.clone(), &q, sources, since).await;
    Json(answer_search(&s.hits, &user, &g, results)).into_response()
}

fn add_answer(status: StatusCode, result: AddResult) -> Response {
    (status, Json(result)).into_response()
}

/// The same gates as a control, then the memo, then the add.
pub(crate) async fn add<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, name)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(req) = serde_json::from_slice::<AddRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match decide(&s.backend.presence(g.clone(), user.clone()).await) {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View => return add_answer(StatusCode::FORBIDDEN, AddResult::NotAllowed),
        Access::Control => {},
    }
    if !s.limiter.allow(&user, Instant::now()) {
        return add_answer(StatusCode::TOO_MANY_REQUESTS, AddResult::TooMany);
    }
    if s.backend.plan(g.clone()).await != PlanView::Premium {
        return add_answer(StatusCode::FORBIDDEN, AddResult::PremiumRequired);
    }
    let Some(track) = s.hits.get(&user, &g, req.hit, Instant::now()) else {
        return add_answer(StatusCode::GONE, AddResult::Expired);
    };
    let at = At::from(req.at);
    match s
        .backend
        .add(user.clone(), &name, g.clone(), track, at)
        .await
    {
        Ok(()) => {
            tracing::info!(guild = %g, user = %user, at = ?at, "dashboard add");
            s.hub.refresh(g.clone()).await;
            let view = s.backend.view(g.clone()).await;
            add_answer(StatusCode::OK, AddResult::Added { view })
        },
        Err(AddRefused::NotPlaying) => add_answer(StatusCode::CONFLICT, AddResult::NotPlaying),
        Err(AddRefused::GameInProgress) => {
            add_answer(StatusCode::LOCKED, AddResult::GameInProgress)
        },
        Err(AddRefused::Failed) => add_answer(StatusCode::INTERNAL_SERVER_ERROR, AddResult::Failed),
    }
}

#[cfg(test)]
mod wire_test {
    use super::*;

    #[test]
    fn sources_and_statuses_have_these_wire_names() {
        let names: Vec<String> = [
            Source::Queue,
            Source::History,
            Source::YouTube,
            Source::SoundCloud,
        ]
        .into_iter()
        .map(|s| serde_json::to_string(&SourceName::from(s)).unwrap())
        .collect();
        assert_eq!(
            names,
            ["\"queue\"", "\"history\"", "\"youtube\"", "\"soundcloud\""]
        );
        let s = serde_json::to_string(&StatusView::TimedOut).unwrap();
        assert_eq!(s, "\"timed_out\"");
    }

    #[test]
    fn the_query_string_parses_or_says_why_not() {
        let ok = parse_search(Some("q=abba&src=history,youtube")).unwrap();
        assert_eq!(ok.0.as_str(), "abba");
        assert_eq!(
            ok.1,
            SourceSet::empty()
                .with(Source::History)
                .with(Source::YouTube)
        );
        for (raw, why) in [
            (None, SEARCH_TOO_SHORT),
            (Some("q=a&src=queue"), SEARCH_TOO_SHORT),
            (Some("q=abba"), SEARCH_NO_SOURCES),
            (Some("q=abba&src="), SEARCH_NO_SOURCES),
            (Some("q=abba&src=queue,spotify"), SEARCH_UNKNOWN_SOURCE),
        ] {
            assert_eq!(parse_search(raw).unwrap_err().0, why, "{raw:?}");
        }
    }

    #[test]
    fn a_hit_view_falls_back_for_a_missing_title_and_keeps_the_detail() {
        let hit = Hit {
            track: SavedTrack {
                url: "https://y/1".into(),
                duration: Some(std::time::Duration::from_secs(61)),
                ..Default::default()
            },
            detail: HitDetail::Queue { position: 3 },
        };
        let v = hit_view(Uuid::from_u128(1), &hit);
        assert_eq!(v.title, QUEUE_NO_TITLE);
        assert_eq!(v.duration_secs, Some(61));
        assert_eq!(
            serde_json::to_string(&v.detail).unwrap(),
            r#"{"kind":"queue","position":3}"#
        );
    }

    #[test]
    fn the_add_body_is_exact() {
        let ok: AddRequest =
            serde_json::from_str(r#"{"hit":"00000000-0000-0000-0000-000000000001","at":"next"}"#)
                .unwrap();
        assert_eq!((ok.hit, ok.at), (Uuid::from_u128(1), AtView::Next));
        assert!(serde_json::from_str::<AddRequest>(
            r#"{"hit":"00000000-0000-0000-0000-000000000001","at":"top"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<AddRequest>(
            r#"{"hit":"00000000-0000-0000-0000-000000000001","at":"end","url":"x"}"#
        )
        .is_err());
    }
}

#[cfg(test)]
mod route_test {
    use super::*;
    use crate::{
        access::Membership,
        backend::{AddRefused, Source, SourceResult, SourceSet},
        routes::router,
        test_support::*,
        view::{PlanView, QueueView},
    };
    use axum::{
        body::Body,
        http::{header, Request, StatusCode},
    };
    use tower::ServiceExt;

    fn search_req(uri: &str, cookie: &str) -> Request<Body> {
        Request::get(uri)
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap()
    }

    async fn get_search(
        fake: std::sync::Arc<FakeBackend>,
        uri: &str,
        cookie: Option<&str>,
    ) -> axum::response::Response {
        let mut req = Request::get(uri);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        app(fake)
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    fn add_req(cookie: &str, text: &str) -> Request<Body> {
        raw_add(Some(cookie), Some("application/json"), Some(ORIGIN), text)
    }

    fn control_req(cookie: &str) -> Request<Body> {
        Request::post("/g/5/control")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, ORIGIN)
            .body(Body::from(
                r#"{"type":"skip","id":"00000000-0000-0000-0000-000000000001"}"#,
            ))
            .unwrap()
    }

    const Q: &str = "/g/5/search?q=abba&src=queue,youtube";

    fn found(url: &str) -> Hit {
        Hit {
            track: SavedTrack {
                url: url.into(),
                title: Some("t".into()),
                ..Default::default()
            },
            detail: HitDetail::Found,
        }
    }

    #[derive(serde::Deserialize)]
    struct SearchAnswer {
        sources: Vec<SourceAnswer>,
    }
    #[derive(serde::Deserialize)]
    struct SourceAnswer {
        source: String,
        status: String,
        hits: Vec<HitAnswer>,
    }
    #[derive(serde::Deserialize)]
    struct HitAnswer {
        id: Uuid,
        url: String,
    }
    #[derive(serde::Deserialize)]
    struct AddAnswer {
        result: String,
    }

    #[tokio::test]
    async fn a_member_searches_and_gets_ids_per_source() {
        let fake = FakeBackend::new(Membership::Member, None, QueueView::Idle);
        *fake.search_result.lock().unwrap() = vec![
            (Source::Queue, SourceResult::Hidden),
            (
                Source::YouTube,
                SourceResult::Hits(vec![found("https://y/1"), found("https://y/2")]),
            ),
        ];
        let r = get_search(fake.clone(), Q, Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let a: SearchAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(
            (a.sources[0].source.as_str(), a.sources[0].status.as_str()),
            ("queue", "hidden")
        );
        assert_eq!(
            (a.sources[1].source.as_str(), a.sources[1].status.as_str()),
            ("youtube", "ok")
        );
        assert_eq!(
            a.sources[1]
                .hits
                .iter()
                .map(|h| h.url.as_str())
                .collect::<Vec<_>>(),
            ["https://y/1", "https://y/2"]
        );
        let (g, text, set, _) = fake.searches.lock().unwrap()[0].clone();
        assert_eq!(
            (g, text.as_str(), set),
            (
                guild(5),
                "abba",
                SourceSet::empty().with(Source::Queue).with(Source::YouTube)
            )
        );
    }

    #[tokio::test]
    async fn search_gates_answer_before_the_backend_searches() {
        // (cookie, membership, uri, want)
        let cases = [
            (None, Membership::Member, Q, StatusCode::UNAUTHORIZED),
            (
                Some(session(9)),
                Membership::NotMember,
                Q,
                StatusCode::NOT_FOUND,
            ),
            (
                Some(session(9)),
                Membership::Unknown,
                Q,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                Some(session(9)),
                Membership::Member,
                "/g/5/search?q=a&src=queue",
                StatusCode::BAD_REQUEST,
            ),
            (
                Some(session(9)),
                Membership::Member,
                "/g/5/search?q=abba&src=spotify",
                StatusCode::BAD_REQUEST,
            ),
            (
                Some(session(9)),
                Membership::Member,
                "/g/0/search?q=abba&src=queue",
                StatusCode::NOT_FOUND,
            ),
        ];
        for (cookie, m, uri, want) in cases {
            let fake = FakeBackend::new(m, None, QueueView::Idle);
            let r = get_search(fake.clone(), uri, cookie.as_deref()).await;
            assert_eq!(r.status(), want, "{uri} {m:?}");
            assert!(
                fake.searches.lock().unwrap().is_empty(),
                "no search for {want}"
            );
        }
    }

    #[tokio::test]
    async fn a_refused_query_answers_with_an_error_body() {
        let fake = FakeBackend::new(Membership::Member, None, QueueView::Idle);
        let r = get_search(fake, "/g/5/search?q=a&src=queue", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        #[derive(serde::Deserialize)]
        struct ErrorAnswer {
            error: String,
        }
        let e: ErrorAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(e.error, SEARCH_TOO_SHORT);
    }

    #[tokio::test]
    async fn the_history_window_follows_the_plan() {
        for (plan, windowed) in [(PlanView::Free, true), (PlanView::Premium, false)] {
            let fake = FakeBackend::new(Membership::Member, None, QueueView::Idle);
            *fake.plan.lock().unwrap() = plan;
            get_search(fake.clone(), Q, Some(&session(9))).await;
            let since = fake.searches.lock().unwrap()[0].3;
            assert_eq!(since.is_some(), windowed, "{plan:?}");
            if let Some(s) = since {
                let age = chrono::Utc::now() - s;
                assert!(
                    (age - crack_core::guild::plan::FREE_HISTORY_WINDOW)
                        .num_seconds()
                        .abs()
                        < 5
                );
            }
        }
    }

    #[tokio::test]
    async fn searching_is_rate_limited_on_its_own_limiter() {
        let fake = controller();
        let app = router(state(fake.clone()));
        for _ in 0..crate::limit::SEARCHES_PER_WINDOW {
            assert_eq!(
                app.clone()
                    .oneshot(search_req(Q, &session(9)))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        }
        assert_eq!(
            app.clone()
                .oneshot(search_req(Q, &session(9)))
                .await
                .unwrap()
                .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            fake.searches.lock().unwrap().len(),
            crate::limit::SEARCHES_PER_WINDOW,
            "the limited search never reached the backend"
        );
        // The controls limiter is untouched: a control still goes through.
        let r = app.clone().oneshot(control_req(&session(9))).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(fake.control_count(), 1);
    }

    #[tokio::test]
    async fn a_hit_found_by_search_is_what_add_queues() {
        let fake = FakeBackend::new(Membership::Member, Some(bot_channel()), playing());
        *fake.search_result.lock().unwrap() = vec![(
            Source::YouTube,
            SourceResult::Hits(vec![found("https://y/1"), found("https://y/2")]),
        )];
        let app = router(state(fake.clone()));
        let a: SearchAnswer = serde_json::from_str(
            &body(
                app.clone()
                    .oneshot(search_req(Q, &session(9)))
                    .await
                    .unwrap(),
            )
            .await,
        )
        .unwrap();
        let id = a.sources[0].hits[1].id;
        let r = app
            .clone()
            .oneshot(add_req(
                &session(9),
                &format!(r#"{{"hit":"{id}","at":"next"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let ans: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(ans.result, "added");
        let adds = fake.adds.lock().unwrap();
        assert_eq!(adds.len(), 1);
        let (g, who, name, track, at) = &adds[0];
        assert_eq!(
            (
                g.clone(),
                who.clone(),
                name.as_str(),
                track.url.as_str(),
                *at
            ),
            (guild(5), user(9), "tester", "https://y/2", At::Next)
        );
    }

    #[tokio::test]
    async fn an_unknown_or_foreign_hit_is_gone() {
        let fake = FakeBackend::new(Membership::Member, Some(bot_channel()), playing());
        *fake.search_result.lock().unwrap() = vec![(
            Source::YouTube,
            SourceResult::Hits(vec![found("https://y/1")]),
        )];
        let app = router(state(fake.clone()));
        let a: SearchAnswer = serde_json::from_str(
            &body(
                app.clone()
                    .oneshot(search_req(Q, &session(9)))
                    .await
                    .unwrap(),
            )
            .await,
        )
        .unwrap();
        let id = a.sources[0].hits[0].id;
        // Another member replays the id.
        let r = app
            .clone()
            .oneshot(add_req(
                &session(10),
                &format!(r#"{{"hit":"{id}","at":"end"}}"#),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::GONE);
        let ans: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(ans.result, "expired");
        assert!(fake.adds.lock().unwrap().is_empty());
    }

    /// A search on `app` as member 9, then the id of its first hit.
    async fn first_hit(app: &axum::Router) -> Uuid {
        let r = app
            .clone()
            .oneshot(search_req(Q, &session(9)))
            .await
            .unwrap();
        let a: SearchAnswer = serde_json::from_str(&body(r).await).unwrap();
        a.sources[0].hits[0].id
    }

    fn one_hit(fake: &FakeBackend) {
        *fake.search_result.lock().unwrap() = vec![(
            Source::YouTube,
            SourceResult::Hits(vec![found("https://y/1")]),
        )];
    }

    fn raw_add(
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        text: &str,
    ) -> Request<Body> {
        let mut req = Request::post("/g/5/add");
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        if let Some(c) = ctype {
            req = req.header(header::CONTENT_TYPE, c);
        }
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        req.body(Body::from(text.to_owned())).unwrap()
    }

    #[tokio::test]
    async fn add_gates_in_controls_order_and_add_nothing() {
        let json = Some("application/json");
        // (cookie, ctype, origin, body (HIT = the real id), membership, in voice, plan, status, result)
        type Case = (
            bool,
            Option<&'static str>,
            Option<&'static str>,
            &'static str,
            Membership,
            bool,
            PlanView,
            StatusCode,
            Option<&'static str>,
        );
        let ok_body = r#"{"hit":"HIT","at":"end"}"#;
        let cases: Vec<Case> = vec![
            (
                false,
                json,
                Some(ORIGIN),
                ok_body,
                Membership::Member,
                true,
                PlanView::Premium,
                StatusCode::UNAUTHORIZED,
                None,
            ),
            (
                true,
                Some("text/plain"),
                Some(ORIGIN),
                ok_body,
                Membership::Member,
                true,
                PlanView::Premium,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                None,
            ),
            (
                true,
                json,
                Some("https://evil.example"),
                ok_body,
                Membership::Member,
                true,
                PlanView::Premium,
                StatusCode::FORBIDDEN,
                None,
            ),
            (
                true,
                json,
                None,
                ok_body,
                Membership::Member,
                true,
                PlanView::Premium,
                StatusCode::FORBIDDEN,
                None,
            ),
            (
                true,
                json,
                Some(ORIGIN),
                r#"{"hit":"HIT","at":"top"}"#,
                Membership::Member,
                true,
                PlanView::Premium,
                StatusCode::BAD_REQUEST,
                None,
            ),
            (
                true,
                json,
                Some(ORIGIN),
                r#"{"hit":"HIT","at":"end","url":"https://x"}"#,
                Membership::Member,
                true,
                PlanView::Premium,
                StatusCode::BAD_REQUEST,
                None,
            ),
            (
                true,
                json,
                Some(ORIGIN),
                ok_body,
                Membership::NotMember,
                true,
                PlanView::Premium,
                StatusCode::NOT_FOUND,
                None,
            ),
            (
                true,
                json,
                Some(ORIGIN),
                ok_body,
                Membership::Unknown,
                true,
                PlanView::Premium,
                StatusCode::SERVICE_UNAVAILABLE,
                None,
            ),
            (
                true,
                json,
                Some(ORIGIN),
                ok_body,
                Membership::Member,
                false,
                PlanView::Premium,
                StatusCode::FORBIDDEN,
                Some("not_allowed"),
            ),
            (
                true,
                json,
                Some(ORIGIN),
                ok_body,
                Membership::Member,
                true,
                PlanView::Free,
                StatusCode::FORBIDDEN,
                Some("premium_required"),
            ),
        ];
        for (cookie, ctype, origin, text, membership, in_voice, plan, want, result) in cases {
            let fake = FakeBackend::new(Membership::Member, Some(bot_channel()), playing());
            one_hit(&fake);
            let app = router(state(fake.clone()));
            let id = first_hit(&app).await;
            *fake.membership.lock().unwrap() = membership;
            *fake.user_channel.lock().unwrap() = in_voice.then(bot_channel);
            *fake.plan.lock().unwrap() = plan;
            let s9 = session(9);
            let req = raw_add(
                cookie.then_some(s9.as_str()),
                ctype,
                origin,
                &text.replace("HIT", &id.to_string()),
            );
            let r = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                r.status(),
                want,
                "{ctype:?} {origin:?} {text} {membership:?} {in_voice} {plan:?}"
            );
            if let Some(result) = result {
                let a: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
                assert_eq!(a.result, result);
            }
            assert!(
                fake.adds.lock().unwrap().is_empty(),
                "nothing added for {want}"
            );
        }
    }

    #[tokio::test]
    async fn add_refusals_map_to_their_answers() {
        for (refused, status, result) in [
            (AddRefused::NotPlaying, StatusCode::CONFLICT, "not_playing"),
            (
                AddRefused::GameInProgress,
                StatusCode::LOCKED,
                "game_in_progress",
            ),
            (
                AddRefused::Failed,
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed",
            ),
        ] {
            let fake = FakeBackend::new(Membership::Member, Some(bot_channel()), playing());
            one_hit(&fake);
            *fake.add_result.lock().unwrap() = Err(refused);
            let app = router(state(fake.clone()));
            let id = first_hit(&app).await;
            let r = app
                .clone()
                .oneshot(add_req(
                    &session(9),
                    &format!(r#"{{"hit":"{id}","at":"end"}}"#),
                ))
                .await
                .unwrap();
            assert_eq!(r.status(), status, "{refused:?}");
            let a: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, result);
        }
    }

    /// Search as member 9 and return every hit with its source.
    async fn search_all(app: &axum::Router) -> Vec<(String, HitAnswer)> {
        let r = app
            .clone()
            .oneshot(search_req(Q, &session(9)))
            .await
            .unwrap();
        let a: SearchAnswer = serde_json::from_str(&body(r).await).unwrap();
        a.sources
            .into_iter()
            .flat_map(|s| s.hits.into_iter().map(move |h| (s.source.clone(), h)))
            .collect()
    }

    async fn add_hit(app: &axum::Router, id: Uuid, at: &str) -> axum::response::Response {
        app.clone()
            .oneshot(add_req(
                &session(9),
                &format!(r#"{{"hit":"{id}","at":"{at}"}}"#),
            ))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn every_hit_of_every_source_adds_its_own_track() {
        let fake = FakeBackend::new(Membership::Member, Some(bot_channel()), playing());
        *fake.search_result.lock().unwrap() = vec![
            (
                Source::Queue,
                SourceResult::Hits(vec![found("https://q/1"), found("https://q/2")]),
            ),
            (
                Source::YouTube,
                SourceResult::Hits(vec![found("https://y/1"), found("https://y/2")]),
            ),
        ];
        let app = router(state(fake.clone()));
        let hits = search_all(&app).await;
        assert_eq!(hits.len(), 4);
        for (source, h) in &hits {
            let before = fake.adds.lock().unwrap().len();
            assert_eq!(add_hit(&app, h.id, "end").await.status(), StatusCode::OK);
            let adds = fake.adds.lock().unwrap();
            assert_eq!(adds.len(), before + 1);
            assert_eq!(adds[before].3.url, h.url, "{source} {}", h.url);
        }
    }

    fn after_add() -> QueueView {
        let mut v = playing();
        if let QueueView::Playing { upcoming, rev, .. } = &mut v {
            upcoming.pop();
            *rev += 1;
        }
        v
    }

    #[tokio::test(start_paused = true)]
    async fn an_add_answers_with_the_new_view_and_reaches_open_watchers_at_once() {
        use crate::watch::TICK;
        let fake = FakeBackend::new(Membership::Member, Some(bot_channel()), playing());
        one_hit(&fake);
        *fake.view_after_add.lock().unwrap() = Some(after_add());
        let st = state(fake.clone());
        let app = router(st.clone());
        let id = first_hit(&app).await;
        let mut rx = st.hub.subscribe(guild(5)).await;
        rx.borrow_and_update();
        let r = add_hit(&app, id, "end").await;
        assert_eq!(r.status(), StatusCode::OK);
        #[derive(serde::Deserialize)]
        struct Added {
            result: String,
            view: QueueView,
        }
        let a: Added = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "added");
        assert_eq!(a.view, after_add(), "the answer is read after the add");
        tokio::time::timeout(TICK / 2, rx.changed())
            .await
            .expect("published without waiting for a tick")
            .unwrap();
        assert_eq!(**rx.borrow(), after_add(), "watchers see the new queue");
    }

    #[tokio::test]
    async fn an_add_spends_the_controls_budget() {
        let fake = controller();
        one_hit(&fake);
        let app = router(state(fake.clone()));
        let id = first_hit(&app).await;
        for _ in 0..crate::limit::CONTROLS_PER_WINDOW {
            let r = app.clone().oneshot(control_req(&session(9))).await.unwrap();
            assert_eq!(r.status(), StatusCode::OK);
        }
        let r = add_hit(&app, id, "end").await;
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        let a: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "too_many");
        assert!(fake.adds.lock().unwrap().is_empty());
    }

    /// Rows that break two gates at once: the earlier gate answers.
    #[tokio::test]
    async fn the_earlier_of_two_broken_gates_answers() {
        let json = Some("application/json");
        let bad = "not json";
        let unknown = "00000000-0000-0000-0000-0000000000ff";
        // (what, cookie, ctype, origin, body, in voice, plan, exhaust limiter, status, result)
        type Row = (
            &'static str,
            bool,
            Option<&'static str>,
            Option<&'static str>,
            String,
            bool,
            PlanView,
            bool,
            StatusCode,
            Option<&'static str>,
        );
        let rows: Vec<Row> = vec![
            (
                "no session beats a bad content type",
                false,
                Some("text/plain"),
                Some(ORIGIN),
                bad.into(),
                true,
                PlanView::Premium,
                false,
                StatusCode::UNAUTHORIZED,
                None,
            ),
            (
                "content type beats origin",
                true,
                Some("text/plain"),
                Some("https://evil.example"),
                bad.into(),
                true,
                PlanView::Premium,
                false,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                None,
            ),
            (
                "origin beats the body",
                true,
                json,
                Some("https://evil.example"),
                bad.into(),
                true,
                PlanView::Premium,
                false,
                StatusCode::FORBIDDEN,
                None,
            ),
            (
                "view-only beats premium",
                true,
                json,
                Some(ORIGIN),
                "HIT".into(),
                false,
                PlanView::Free,
                false,
                StatusCode::FORBIDDEN,
                Some("not_allowed"),
            ),
            (
                "the limiter beats premium",
                true,
                json,
                Some(ORIGIN),
                "HIT".into(),
                true,
                PlanView::Free,
                true,
                StatusCode::TOO_MANY_REQUESTS,
                Some("too_many"),
            ),
            (
                "premium beats an unknown hit",
                true,
                json,
                Some(ORIGIN),
                format!(r#"{{"hit":"{unknown}","at":"end"}}"#),
                true,
                PlanView::Free,
                false,
                StatusCode::FORBIDDEN,
                Some("premium_required"),
            ),
            (
                "an unknown hit is gone, not a premium problem",
                true,
                json,
                Some(ORIGIN),
                format!(r#"{{"hit":"{unknown}","at":"end"}}"#),
                true,
                PlanView::Premium,
                false,
                StatusCode::GONE,
                Some("expired"),
            ),
        ];
        for (what, cookie, ctype, origin, text, in_voice, plan, exhaust, want, result) in rows {
            let fake = FakeBackend::new(Membership::Member, Some(bot_channel()), playing());
            one_hit(&fake);
            let app = router(state(fake.clone()));
            let id = first_hit(&app).await;
            if exhaust {
                for _ in 0..crate::limit::CONTROLS_PER_WINDOW {
                    app.clone().oneshot(control_req(&session(9))).await.unwrap();
                }
            }
            *fake.user_channel.lock().unwrap() = in_voice.then(bot_channel);
            *fake.plan.lock().unwrap() = plan;
            let s9 = session(9);
            let req = raw_add(
                cookie.then_some(s9.as_str()),
                ctype,
                origin,
                &text.replace("HIT", &format!(r#"{{"hit":"{id}","at":"end"}}"#)),
            );
            let r = app.clone().oneshot(req).await.unwrap();
            assert_eq!(r.status(), want, "{what}");
            if let Some(result) = result {
                let a: AddAnswer = serde_json::from_str(&body(r).await).unwrap();
                assert_eq!(a.result, result, "{what}");
            }
            assert!(
                fake.adds.lock().unwrap().is_empty(),
                "{what}: nothing added"
            );
        }
    }
}
