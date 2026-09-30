//! The dashboard's HTTP surface.

use crate::{
    access::{decide, Access},
    backend::{Backend, MoveRefused},
    page,
    view::{MoveRequest, MoveResult, PageState, QueueView},
    watch::Hub,
};
use axum::{
    body::Bytes,
    extract::{FromRef, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Redirect, Response,
    },
    routing::{get, post},
    Json, Router,
};
use catacombs::auth::AuthenticatedUser;
use serenity::all::{GuildId, UserId};
use std::{convert::Infallible, num::NonZeroU64, sync::Arc, time::Duration};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tower_http::timeout::TimeoutLayer;

/// Everything but the stylesheet and two scripts is off; no framing.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// How long any non-streaming request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// How often an open stream re-checks who is watching.
pub const RECHECK: Duration = Duration::from_secs(5);
/// SSE comment interval, so proxies do not idle the stream out.
pub const KEEPALIVE: Duration = Duration::from_secs(15);

pub struct WebState<B: Backend> {
    pub auth: Arc<catacombs::AppState>,
    pub backend: Arc<B>,
    pub hub: Arc<Hub<B>>,
    /// The only `Origin` a move is accepted from.
    pub origin: Arc<str>,
}

impl<B: Backend> Clone for WebState<B> {
    fn clone(&self) -> Self {
        Self {
            auth: self.auth.clone(),
            backend: self.backend.clone(),
            hub: self.hub.clone(),
            origin: self.origin.clone(),
        }
    }
}

impl<B: Backend> FromRef<WebState<B>> for Arc<catacombs::AppState> {
    fn from_ref(s: &WebState<B>) -> Self {
        s.auth.clone()
    }
}

/// The session's user, or `None` if the session is missing or invalid.
type Session = Result<AuthenticatedUser, StatusCode>;

fn user_id(s: Session) -> Option<(UserId, String)> {
    let u = s.ok()?;
    let id = NonZeroU64::new(u64::try_from(u.user_id).ok()?)?;
    Some((UserId::new(id.get()), u.username))
}

/// `/g/0`, `/g/abc` and overflow are not guilds. `GuildId::new(0)` panics.
pub(crate) fn parse_guild(raw: &str) -> Option<GuildId> {
    raw.parse::<NonZeroU64>()
        .ok()
        .map(|n| GuildId::new(n.get()))
}

fn login_redirect(return_to: &str) -> Response {
    let q = serde_urlencoded::to_string([("return_to", return_to)]).expect("a plain string");
    Redirect::to(&format!("/auth/login?{q}")).into_response()
}

pub(crate) fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Html(page::message_page("Not found", "There is nothing here.")),
    )
        .into_response()
}

pub(crate) fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(page::message_page(
            "Try again",
            "Discord did not answer. Try again in a moment.",
        )),
    )
        .into_response()
}

async fn picker<B: Backend>(State(s): State<WebState<B>>, session: Session) -> Response {
    let Some((user, name)) = user_id(session) else {
        return login_redirect("/");
    };
    let guilds = s.backend.guilds_for(user).await;
    Html(page::picker_page(&name, &guilds)).into_response()
}

async fn guild_page<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, _)) = user_id(session) else {
        return login_redirect(&format!("/g/{g}"));
    };
    let access = decide(&s.backend.presence(g, user).await);
    match access {
        Access::Hidden => not_found(),
        Access::Unavailable => unavailable(),
        Access::View | Access::Control => {
            let view = s.backend.view(g).await;
            let name = s
                .backend
                .guild_name(g)
                .unwrap_or_else(|| "Server".to_owned());
            let state = PageState {
                view: &view,
                can_control: access == Access::Control,
            };
            Html(page::queue_page(&name, g, &state)).into_response()
        },
    }
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
        })
}

fn answer(status: StatusCode, result: MoveResult) -> Response {
    (status, Json(result)).into_response()
}

async fn move_track<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    // Cookie auth means a cross-site POST would carry the cookie. SameSite=Lax
    // withholds it; these two refuse it independently: no HTML form can send
    // JSON, and a foreign page cannot forge Origin.
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(req) = serde_json::from_slice::<MoveRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match decide(&s.backend.presence(g, user).await) {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View => return answer(StatusCode::FORBIDDEN, MoveResult::NotAllowed),
        Access::Control => {},
    }
    match s.backend.move_track(g, req.id, req.to).await {
        Ok(to) => {
            tracing::info!(guild = %g, user = %user, track = %req.id, to, "dashboard move");
            let view = s.backend.view(g).await;
            s.hub.publish(g, view.clone()).await;
            answer(StatusCode::OK, MoveResult::Moved { view })
        },
        Err(MoveRefused::Absent | MoveRefused::NowPlaying) => {
            let view = s.backend.view(g).await;
            answer(StatusCode::CONFLICT, MoveResult::Conflict { view })
        },
        Err(MoveRefused::GameInProgress) => answer(StatusCode::LOCKED, MoveResult::GameInProgress),
        Err(MoveRefused::NotPlaying) => answer(StatusCode::CONFLICT, MoveResult::NotPlaying),
    }
}

fn event(view: &QueueView, can_control: bool) -> Event {
    Event::default()
        .data(serde_json::to_string(&PageState { view, can_control }).expect("the view serializes"))
}

async fn events<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    // EventSource cannot follow a login redirect: a plain 401.
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let access = decide(&s.backend.presence(g, user).await);
    match access {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View | Access::Control => {},
    }
    let mut rx = s.hub.subscribe(g).await;
    let (tx, out) = tokio::sync::mpsc::channel::<Event>(8);
    let backend = s.backend.clone();
    tokio::spawn(async move {
        let mut can_control = access == Access::Control;
        let mut last = rx.borrow_and_update().clone();
        if tx.send(event(&last, can_control)).await.is_err() {
            return;
        }
        let mut recheck = tokio::time::interval(RECHECK);
        recheck.tick().await;
        loop {
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    last = rx.borrow_and_update().clone();
                    if tx.send(event(&last, can_control)).await.is_err() {
                        return;
                    }
                },
                _ = recheck.tick() => match decide(&backend.presence(g, user).await) {
                    // Left the guild: close the stream.
                    Access::Hidden => return,
                    // Cannot tell right now: keep what we had.
                    Access::Unavailable => {},
                    now => {
                        let now = now == Access::Control;
                        if now != can_control {
                            can_control = now;
                            if tx.send(event(&last, can_control)).await.is_err() {
                                return;
                            }
                        }
                    },
                },
                () = tx.closed() => return,
            }
        }
    });
    let stream = ReceiverStream::new(out).map(Ok::<_, Infallible>);
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(KEEPALIVE))
        .into_response()
}

async fn asset(Path(name): Path<String>) -> Response {
    let (body, ctype): (&'static str, &'static str) = match name.as_str() {
        "app.js" => (
            include_str!("../assets/app.js"),
            "text/javascript; charset=utf-8",
        ),
        "sortable.min.js" => (
            include_str!("../assets/sortable.min.js"),
            "text/javascript; charset=utf-8",
        ),
        "app.css" => (include_str!("../assets/app.css"), "text/css; charset=utf-8"),
        _ => return not_found(),
    };
    (
        [
            (header::CONTENT_TYPE, ctype),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

async fn security_headers(mut resp: Response) -> Response {
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    resp
}

/// The whole dashboard: pages, the stream, moves, assets, and catacombs at
/// `/auth`. Everything but the event stream has a timeout.
pub fn router<B: Backend>(state: WebState<B>) -> Router {
    let timeout = TimeoutLayer::with_status_code(StatusCode::SERVICE_UNAVAILABLE, REQUEST_TIMEOUT);
    let timed = Router::new()
        .route("/", get(picker::<B>))
        .route("/g/{guild}", get(guild_page::<B>))
        .route("/assets/{name}", get(asset))
        .route("/g/{guild}/move", post(move_track::<B>))
        .layer(timeout);
    Router::new()
        .merge(timed)
        // Outside the timeout: a stream is meant to stay open.
        .route("/g/{guild}/events", get(events::<B>))
        .nest(
            "/auth",
            // /auth/callback and /auth/exchange call Discord through a client
            // with no timeout of its own, so this layer is the only bound.
            catacombs::routes::auth_router()
                .with_state(state.auth.clone())
                .layer(timeout),
        )
        .with_state(state)
        .layer(axum::middleware::map_response(security_headers))
}

#[cfg(test)]
mod test {
    use crate::{access::Membership, test_support::*, view::QueueView};
    use axum::{
        body::Body,
        http::{header, Request, StatusCode},
    };
    use tower::ServiceExt;

    async fn get(
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

    fn member_viewing() -> std::sync::Arc<FakeBackend> {
        FakeBackend::new(Membership::Member, None, QueueView::Idle)
    }

    #[tokio::test]
    async fn signed_out_visitors_are_sent_to_login_and_back() {
        let r = get(member_viewing(), "/", None).await;
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        assert_eq!(r.headers()[header::LOCATION], "/auth/login?return_to=%2F");
        let r = get(member_viewing(), "/g/5", None).await;
        assert_eq!(
            r.headers()[header::LOCATION],
            "/auth/login?return_to=%2Fg%2F5"
        );
    }

    #[tokio::test]
    async fn the_picker_lists_the_backends_guilds() {
        let r = get(member_viewing(), "/", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let html = body(r).await;
        assert!(html.contains("href=\"/g/5\"") && html.contains("Five"));
    }

    #[tokio::test]
    async fn malformed_guild_ids_are_404_not_a_panic() {
        for uri in ["/g/0", "/g/abc", "/g/99999999999999999999", "/g/-1"] {
            let r = get(member_viewing(), uri, Some(&session(9))).await;
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn a_non_member_gets_404_and_an_unknown_gets_503() {
        let r = get(
            FakeBackend::new(Membership::NotMember, None, QueueView::Idle),
            "/g/5",
            Some(&session(9)),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let r = get(
            FakeBackend::new(Membership::Unknown, None, QueueView::Idle),
            "/g/5",
            Some(&session(9)),
        )
        .await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn the_page_inlines_the_view_and_this_viewers_permission() {
        #[derive(serde::Deserialize)]
        struct State {
            view: QueueView,
            can_control: bool,
        }
        async fn state_of(fake: std::sync::Arc<FakeBackend>) -> State {
            let html = body(get(fake, "/g/5", Some(&session(9))).await).await;
            let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
            let end = start + html[start..].find("</script>").unwrap();
            serde_json::from_str(&html[start..end]).unwrap()
        }
        let viewer = state_of(member_viewing()).await;
        assert_eq!(viewer.view, QueueView::Idle);
        assert!(!viewer.can_control);
        let controller = state_of(FakeBackend::new(
            Membership::Member,
            Some(BOT_CHANNEL),
            QueueView::Idle,
        ))
        .await;
        assert!(controller.can_control);
    }

    #[test]
    fn the_csp_allows_nothing_unsafe() {
        assert!(!crate::routes::CSP.contains("unsafe-"));
    }

    #[tokio::test]
    async fn every_response_carries_the_security_headers() {
        // A page, a catacombs route, and a path nothing matches (the fallback 404).
        for uri in ["/g/5", "/auth/login", "/nowhere"] {
            let r = get(member_viewing(), uri, Some(&session(9))).await;
            assert_eq!(
                r.headers()["content-security-policy"],
                crate::routes::CSP,
                "{uri}"
            );
            assert_eq!(r.headers()["x-content-type-options"], "nosniff", "{uri}");
            assert_eq!(r.headers()["referrer-policy"], "same-origin", "{uri}");
        }
    }

    #[tokio::test]
    async fn assets_are_served_by_name_only() {
        let r = get(member_viewing(), "/assets/app.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/javascript"));
        let r = get(member_viewing(), "/assets/sortable.min.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        for uri in ["/assets/nope.js", "/assets/..%2FCargo.toml"] {
            assert_eq!(
                get(member_viewing(), uri, None).await.status(),
                StatusCode::NOT_FOUND,
                "{uri}"
            );
        }
    }

    use crate::backend::MoveRefused;
    use crate::routes::RECHECK;
    use crate::view::TrackView;
    use serenity::all::GuildId;
    use uuid::Uuid;

    fn playing() -> QueueView {
        let t = |n| TrackView {
            id: Uuid::from_u128(n),
            title: format!("t{n}"),
            url: None,
            duration_secs: None,
            requester: None,
        };
        QueueView::Playing {
            now: t(1),
            upcoming: vec![t(2), t(3)],
            rev: 7,
        }
    }

    fn controller() -> std::sync::Arc<FakeBackend> {
        FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), playing())
    }

    const MOVE: &str = r#"{"id":"00000000-0000-0000-0000-000000000003","to":0}"#;

    async fn post_move(
        fake: std::sync::Arc<FakeBackend>,
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        body_text: &str,
    ) -> axum::response::Response {
        let mut req = Request::post("/g/5/move");
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        if let Some(c) = ctype {
            req = req.header(header::CONTENT_TYPE, c);
        }
        if let Some(o) = origin {
            req = req.header(header::ORIGIN, o);
        }
        app(fake)
            .oneshot(req.body(Body::from(body_text.to_owned())).unwrap())
            .await
            .unwrap()
    }

    #[derive(serde::Deserialize)]
    struct Answer {
        result: String,
        view: Option<QueueView>,
    }

    #[tokio::test]
    async fn a_controller_moves_and_the_backend_gets_exactly_that_move() {
        let fake = controller();
        let r = post_move(
            fake.clone(),
            Some(&session(9)),
            Some("application/json"),
            Some(ORIGIN),
            MOVE,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "moved");
        assert_eq!(a.view, Some(playing()));
        assert_eq!(
            *fake.moves.lock().unwrap(),
            vec![(GuildId::new(5), Uuid::from_u128(3), 0)]
        );
    }

    #[tokio::test]
    async fn refusals_before_the_backend_move_nothing() {
        type Case<'a> = (
            Option<String>,
            Option<&'a str>,
            Option<&'a str>,
            &'a str,
            StatusCode,
        );
        let cases: [Case; 6] = [
            (
                None,
                Some("application/json"),
                Some(ORIGIN),
                MOVE,
                StatusCode::UNAUTHORIZED,
            ),
            (
                Some(session(9)),
                Some("text/plain"),
                Some(ORIGIN),
                MOVE,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                Some(session(9)),
                None,
                Some(ORIGIN),
                MOVE,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                Some(session(9)),
                Some("application/json"),
                Some("https://evil.example"),
                MOVE,
                StatusCode::FORBIDDEN,
            ),
            (
                Some(session(9)),
                Some("application/json"),
                None,
                MOVE,
                StatusCode::FORBIDDEN,
            ),
            (
                Some(session(9)),
                Some("application/json"),
                Some(ORIGIN),
                "{not json",
                StatusCode::BAD_REQUEST,
            ),
        ];
        for (cookie, ctype, origin, text, want) in cases {
            let fake = controller();
            let r = post_move(fake.clone(), cookie.as_deref(), ctype, origin, text).await;
            assert_eq!(r.status(), want, "{ctype:?} {origin:?} {text}");
            assert_eq!(fake.move_count(), 0, "nothing moved for {want}");
        }
    }

    #[tokio::test]
    async fn a_viewer_not_in_the_bots_channel_is_not_allowed() {
        for channel in [None, Some(serenity::all::ChannelId::new(1))] {
            let fake = FakeBackend::new(Membership::Member, channel, playing());
            let r = post_move(
                fake.clone(),
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                MOVE,
            )
            .await;
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
            let a: Answer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, "not_allowed");
            assert_eq!(fake.move_count(), 0);
        }
    }

    #[tokio::test]
    async fn a_non_member_gets_404_even_to_a_move() {
        let fake = FakeBackend::new(Membership::NotMember, Some(BOT_CHANNEL), playing());
        let r = post_move(
            fake.clone(),
            Some(&session(9)),
            Some("application/json"),
            Some(ORIGIN),
            MOVE,
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert_eq!(fake.move_count(), 0);
    }

    #[tokio::test]
    async fn backend_refusals_map_to_their_answers() {
        let cases = [
            (MoveRefused::Absent, StatusCode::CONFLICT, "conflict", true),
            (
                MoveRefused::NowPlaying,
                StatusCode::CONFLICT,
                "conflict",
                true,
            ),
            (
                MoveRefused::GameInProgress,
                StatusCode::LOCKED,
                "game_in_progress",
                false,
            ),
            (
                MoveRefused::NotPlaying,
                StatusCode::CONFLICT,
                "not_playing",
                false,
            ),
        ];
        for (refusal, status, result, has_view) in cases {
            let fake = controller();
            *fake.move_result.lock().unwrap() = Err(refusal);
            let r = post_move(
                fake,
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                MOVE,
            )
            .await;
            assert_eq!(r.status(), status, "{refusal:?}");
            let a: Answer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, result);
            assert_eq!(a.view.is_some(), has_view, "{refusal:?}");
        }
    }

    #[tokio::test]
    async fn a_move_reaches_open_watchers_at_once() {
        let fake = controller();
        let st = state(fake.clone());
        let mut rx = st.hub.subscribe(GuildId::new(5)).await;
        rx.borrow_and_update();
        *fake.view.lock().unwrap() = QueueView::Idle; // what the backend reads after the move
        let req = Request::post("/g/5/move")
            .header(header::COOKIE, session(9))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, ORIGIN)
            .body(Body::from(MOVE))
            .unwrap();
        let r = crate::routes::router(st).oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(
            rx.has_changed().unwrap(),
            "published without waiting for a tick"
        );
        assert_eq!(**rx.borrow(), QueueView::Idle);
    }

    async fn first_event(resp: axum::response::Response) -> String {
        use http_body_util::BodyExt;
        let mut body = resp.into_body();
        let mut text = String::new();
        while !text.contains("\n\n") {
            let frame = tokio::time::timeout(std::time::Duration::from_secs(2), body.frame())
                .await
                .expect("an event within 2s")
                .expect("stream open")
                .unwrap();
            if let Ok(data) = frame.into_data() {
                text.push_str(std::str::from_utf8(&data).unwrap());
            }
        }
        text.lines()
            .find_map(|l| l.strip_prefix("data: "))
            .expect("a data line")
            .to_owned()
    }

    #[tokio::test]
    async fn the_stream_needs_a_session_and_membership() {
        let r = get(controller(), "/g/5/events", None).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let r = get(
            FakeBackend::new(Membership::NotMember, None, playing()),
            "/g/5/events",
            Some(&session(9)),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn the_stream_opens_with_the_current_view_and_permission() {
        #[derive(serde::Deserialize)]
        struct State {
            view: QueueView,
            can_control: bool,
        }
        let r = get(controller(), "/g/5/events", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let s: State = serde_json::from_str(&first_event(r).await).unwrap();
        assert_eq!(s.view, playing());
        assert!(s.can_control);
    }

    #[tokio::test(start_paused = true)]
    async fn leaving_the_voice_channel_drops_control_within_a_recheck() {
        use http_body_util::BodyExt;
        #[derive(serde::Deserialize)]
        struct State {
            can_control: bool,
        }
        let fake = controller();
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let _first = body.frame().await.unwrap().unwrap();
        *fake.user_channel.lock().unwrap() = None;
        tokio::time::sleep(RECHECK + std::time::Duration::from_secs(1)).await;
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        let data = text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        let s: State = serde_json::from_str(data).unwrap();
        assert!(!s.can_control);
    }

    #[tokio::test(start_paused = true)]
    async fn leaving_the_guild_closes_the_stream() {
        use http_body_util::BodyExt;
        let fake = controller();
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let _first = body.frame().await.unwrap().unwrap();
        *fake.membership.lock().unwrap() = Membership::NotMember;
        tokio::time::sleep(RECHECK + std::time::Duration::from_secs(1)).await;
        // Keep-alive comments may arrive first; the stream must end.
        loop {
            match body.frame().await {
                None => break,
                Some(Ok(f)) => assert!(
                    !f.into_data()
                        .map(|d| d.starts_with(b"data:"))
                        .unwrap_or(false),
                    "no more events"
                ),
                Some(Err(e)) => panic!("{e}"),
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_stream_outlives_the_request_timeout() {
        use http_body_util::BodyExt;
        let r = get(controller(), "/g/5/events", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let mut body = r.into_body();
        let _first = body.frame().await.unwrap().unwrap();
        tokio::time::sleep(crate::routes::REQUEST_TIMEOUT + std::time::Duration::from_secs(1))
            .await;
        // Still open: the next frame is the keep-alive, not an end or an error.
        let frame = body.frame().await.expect("stream still open").unwrap();
        assert!(frame.into_data().is_ok());
    }
}
