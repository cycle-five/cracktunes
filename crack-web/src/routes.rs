//! The dashboard's HTTP surface.

use crate::{
    access::{decide, Access, HistoryAccess},
    backend::{Backend, ControlRefused, HistoryError, MoveRefused},
    history::{
        parse_history_query, ErrorBody, HistoryQuery, HISTORY_FAILED, NEEDS_MANAGE, NO_DATABASE,
    },
    limit::RateLimit,
    page,
    settings::{
        self, EconomySaveRefused, EconomySettingsForm, ModLogSaveRefused, ModLogSettingsForm,
        RoomSettingsForm, SaveRefused,
    },
    view::{
        ControlRequest, ControlResult, MoveRequest, MoveResult, PageState, PlanView, QueueView,
    },
    watch::Hub,
};
use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Redirect, Response,
    },
    routing::{get, post},
    Json, Router,
};
use catacombs::auth::AuthenticatedUser;
use crack_core::messaging::messages::{
    DASH_ECONOMY_SAVE_FAILED, DASH_ECONOMY_SETTINGS_NOT_LOADED, DASH_ECONOMY_UNKNOWN_ACTION,
    DASH_ECONOMY_UNKNOWN_CHANNEL, DASH_MODLOG_UNKNOWN_CHANNEL, DASH_SETTINGS_FAILED,
    DASH_SETTINGS_NEEDS_MANAGE, DASH_SETTINGS_NOT_LOADED, DASH_SETTINGS_TOO_MANY,
    DASH_SETTINGS_UNKNOWN_CHANNEL, DASH_SETTINGS_UNREADABLE, MODLOG_SETTINGS_NOT_LOADED,
    MODLOG_SETTINGS_SAVE_FAILED,
};
use serenity::all::{GuildId, UserId};
use std::{
    convert::Infallible,
    num::NonZeroU64,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tower_http::timeout::TimeoutLayer;

/// Everything but the stylesheet and two scripts is off; no framing. Trusted
/// Types with no policy makes a supporting browser throw on any string
/// assigned to an HTML sink (innerHTML, document.write, eval...), so track
/// titles can never be parsed as markup.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'; \
require-trusted-types-for 'script'; trusted-types 'none'";

/// How long any non-streaming request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// How often an open stream re-checks who is watching.
pub const RECHECK: Duration = Duration::from_secs(5);
/// SSE comment interval, so proxies do not idle the stream out.
pub const KEEPALIVE: Duration = Duration::from_secs(15);
/// How long a recheck waits on Discord before it keeps what it had.
pub const PRESENCE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the queue page waits to learn whether to link the history. The
/// link is optional: past this, the page is drawn without it.
pub const HISTORY_LINK_TIMEOUT: Duration = Duration::from_secs(2);

pub struct WebState<B: Backend> {
    pub auth: Arc<catacombs::Auth>,
    pub backend: Arc<B>,
    pub hub: Arc<Hub<B>>,
    /// The only `Origin` a move is accepted from.
    pub origin: Arc<str>,
    /// Dashboard controls, per user.
    pub limiter: Arc<RateLimit>,
    /// Each member's last search results.
    pub hits: Arc<crate::hits::HitMemo>,
    /// Searches, per user; separate from `limiter` so searching never spends a control.
    pub search_limiter: Arc<RateLimit>,
}

impl<B: Backend> Clone for WebState<B> {
    fn clone(&self) -> Self {
        Self {
            auth: self.auth.clone(),
            backend: self.backend.clone(),
            hub: self.hub.clone(),
            origin: self.origin.clone(),
            limiter: self.limiter.clone(),
            hits: self.hits.clone(),
            search_limiter: self.search_limiter.clone(),
        }
    }
}

impl<B: Backend> catacombs::HasAuth for WebState<B> {
    fn auth(&self) -> &catacombs::Auth {
        &self.auth
    }
}

/// The session's user, or `None` if the session is missing or invalid.
pub(crate) type Session = Result<AuthenticatedUser, StatusCode>;

pub(crate) fn user_id(s: Session) -> Option<(UserId, String)> {
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
        // #589: a page, not a redirect to the login. Discord's link preview
        // follows redirects, and this one ended on Discord's OAuth page,
        // whose card it showed instead of ours.
        return Html(page::landing_page(&s.origin)).into_response();
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
                plan: plan_or_free(s.backend.as_ref(), g).await,
            };
            let history_link =
                tokio::time::timeout(HISTORY_LINK_TIMEOUT, s.backend.history_access(g, user))
                    .await
                    .is_ok_and(|a| a == HistoryAccess::Allowed);
            Html(page::queue_page(&name, g, &state, history_link)).into_response()
        },
    }
}

fn forbidden_page() -> Response {
    forbidden_with(NEEDS_MANAGE)
}

fn forbidden_with(text: &'static str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Html(page::message_page("Needs Manage Server", text)),
    )
        .into_response()
}

pub(crate) fn error_json(status: StatusCode, error: &'static str) -> Response {
    (status, Json(ErrorBody { error })).into_response()
}

async fn history_page<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, _)) = user_id(session) else {
        return login_redirect(&format!("/g/{g}/history"));
    };
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return not_found(),
        HistoryAccess::Unavailable => return unavailable(),
        HistoryAccess::Forbidden => return forbidden_page(),
        HistoryAccess::Allowed => {},
    }
    match s.backend.history(g, &HistoryQuery::default()).await {
        Ok(first) => {
            let name = s
                .backend
                .guild_name(g)
                .unwrap_or_else(|| "Server".to_owned());
            Html(page::history_page(&name, g, &first)).into_response()
        },
        Err(HistoryError::NoDatabase) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Html(page::message_page("No history", NO_DATABASE)),
        )
            .into_response(),
        Err(HistoryError::Failed) => unavailable(),
    }
}

async fn history_json<B: Backend>(
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
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return StatusCode::NOT_FOUND.into_response(),
        HistoryAccess::Unavailable => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        HistoryAccess::Forbidden => return error_json(StatusCode::FORBIDDEN, NEEDS_MANAGE),
        HistoryAccess::Allowed => {},
    }
    let q = match parse_history_query(query.as_deref()) {
        Ok(q) => q,
        Err(bad) => return error_json(StatusCode::BAD_REQUEST, bad.0),
    };
    match s.backend.history(g, &q).await {
        Ok(page) => Json(page).into_response(),
        Err(HistoryError::NoDatabase) => error_json(StatusCode::SERVICE_UNAVAILABLE, NO_DATABASE),
        Err(HistoryError::Failed) => error_json(StatusCode::SERVICE_UNAVAILABLE, HISTORY_FAILED),
    }
}

// The settings routes use `history_access`: it IS the Manage Server rule
// (a guild owner or bot owner passes, as they do for history).
async fn settings_page<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return not_found();
    };
    let Some((user, _)) = user_id(session) else {
        return login_redirect(&format!("/g/{g}/settings"));
    };
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return not_found(),
        HistoryAccess::Unavailable => return unavailable(),
        HistoryAccess::Forbidden => return forbidden_with(DASH_SETTINGS_NEEDS_MANAGE),
        HistoryAccess::Allowed => {},
    }
    let view = s.backend.room_settings(g).await;
    let modlog = s.backend.mod_log_settings(g).await;
    let economy = s.backend.economy_settings(g).await;
    let name = s
        .backend
        .guild_name(g)
        .unwrap_or_else(|| "Server".to_owned());
    Html(page::settings_page(&name, g, &view, &modlog, &economy)).into_response()
}

/// JSON only and same-origin, as `/control`; the same Manage Server access
/// as the page (`history_access`); the controls' per-user rate limit (M26).
async fn save_rooms<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return StatusCode::NOT_FOUND.into_response(),
        HistoryAccess::Unavailable => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        HistoryAccess::Forbidden => {
            return error_json(StatusCode::FORBIDDEN, DASH_SETTINGS_NEEDS_MANAGE)
        },
        HistoryAccess::Allowed => {},
    }
    if !s.limiter.allow(user, Instant::now()) {
        return error_json(StatusCode::TOO_MANY_REQUESTS, DASH_SETTINGS_TOO_MANY);
    }
    let Ok(form) = serde_json::from_slice::<RoomSettingsForm>(&body) else {
        return error_json(StatusCode::BAD_REQUEST, DASH_SETTINGS_UNREADABLE);
    };
    match s.backend.save_room_settings(user, g, form).await {
        Ok(view) => Json(view).into_response(),
        Err(SaveRefused::Invalid(e)) => {
            error_json(StatusCode::BAD_REQUEST, settings::invalid_message(e))
        },
        Err(SaveRefused::UnknownChannel) => {
            error_json(StatusCode::BAD_REQUEST, DASH_SETTINGS_UNKNOWN_CHANNEL)
        },
        Err(SaveRefused::Failed) => {
            error_json(StatusCode::SERVICE_UNAVAILABLE, DASH_SETTINGS_FAILED)
        },
        Err(SaveRefused::NotLoaded) => {
            error_json(StatusCode::SERVICE_UNAVAILABLE, DASH_SETTINGS_NOT_LOADED)
        },
    }
}

/// The same ladder as `save_rooms`: the access check comes before the body
/// is parsed, so a non-manager learns nothing from how a body fails.
async fn save_modlog<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return StatusCode::NOT_FOUND.into_response(),
        HistoryAccess::Unavailable => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        HistoryAccess::Forbidden => {
            return error_json(StatusCode::FORBIDDEN, DASH_SETTINGS_NEEDS_MANAGE)
        },
        HistoryAccess::Allowed => {},
    }
    if !s.limiter.allow(user, Instant::now()) {
        return error_json(StatusCode::TOO_MANY_REQUESTS, DASH_SETTINGS_TOO_MANY);
    }
    let Ok(form) = serde_json::from_slice::<ModLogSettingsForm>(&body) else {
        return error_json(StatusCode::BAD_REQUEST, DASH_SETTINGS_UNREADABLE);
    };
    match s.backend.save_mod_log_settings(user, g, form).await {
        Ok(view) => Json(view).into_response(),
        Err(ModLogSaveRefused::Invalid(e)) => {
            error_json(StatusCode::BAD_REQUEST, settings::modlog_invalid_message(e))
        },
        Err(ModLogSaveRefused::UnknownChannel) => {
            error_json(StatusCode::BAD_REQUEST, DASH_MODLOG_UNKNOWN_CHANNEL)
        },
        Err(ModLogSaveRefused::Failed) => {
            error_json(StatusCode::SERVICE_UNAVAILABLE, MODLOG_SETTINGS_SAVE_FAILED)
        },
        Err(ModLogSaveRefused::NotLoaded) => {
            error_json(StatusCode::SERVICE_UNAVAILABLE, MODLOG_SETTINGS_NOT_LOADED)
        },
    }
}

/// The same ladder as `save_modlog`.
async fn save_economy<B: Backend>(
    State(s): State<WebState<B>>,
    session: Session,
    Path(raw): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(g) = parse_guild(&raw) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((user, _)) = user_id(session) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match s.backend.history_access(g, user).await {
        HistoryAccess::Hidden => return StatusCode::NOT_FOUND.into_response(),
        HistoryAccess::Unavailable => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        HistoryAccess::Forbidden => {
            return error_json(StatusCode::FORBIDDEN, DASH_SETTINGS_NEEDS_MANAGE)
        },
        HistoryAccess::Allowed => {},
    }
    if !s.limiter.allow(user, Instant::now()) {
        return error_json(StatusCode::TOO_MANY_REQUESTS, DASH_SETTINGS_TOO_MANY);
    }
    let Ok(form) = serde_json::from_slice::<EconomySettingsForm>(&body) else {
        return error_json(StatusCode::BAD_REQUEST, DASH_SETTINGS_UNREADABLE);
    };
    match s.backend.save_economy_settings(user, g, form).await {
        Ok(view) => Json(view).into_response(),
        Err(EconomySaveRefused::Invalid(e)) => error_json(
            StatusCode::BAD_REQUEST,
            settings::economy_invalid_message(e),
        ),
        Err(EconomySaveRefused::UnknownAction) => {
            error_json(StatusCode::BAD_REQUEST, DASH_ECONOMY_UNKNOWN_ACTION)
        },
        Err(EconomySaveRefused::UnknownChannel) => {
            error_json(StatusCode::BAD_REQUEST, DASH_ECONOMY_UNKNOWN_CHANNEL)
        },
        Err(EconomySaveRefused::Failed) => {
            error_json(StatusCode::SERVICE_UNAVAILABLE, DASH_ECONOMY_SAVE_FAILED)
        },
        Err(EconomySaveRefused::NotLoaded) => error_json(
            StatusCode::SERVICE_UNAVAILABLE,
            DASH_ECONOMY_SETTINGS_NOT_LOADED,
        ),
    }
}

pub(crate) fn is_json(headers: &HeaderMap) -> bool {
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
    match s.backend.move_track(user, g, req.id, req.to).await {
        Ok(to) => {
            tracing::info!(guild = %g, user = %user, track = %req.id, to, "dashboard move");
            // Open tabs follow through the poller, which reads after this;
            // the mover gets a read of its own.
            s.hub.refresh(g).await;
            let view = s.backend.view(g).await;
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

fn control_answer(status: StatusCode, result: ControlResult) -> Response {
    (status, Json(result)).into_response()
}

/// The same gates as a move, then the rate limit, then the plan.
async fn control<B: Backend>(
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
    if !is_json(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&*s.origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(req) = serde_json::from_slice::<ControlRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match decide(&s.backend.presence(g, user).await) {
        Access::Hidden => return not_found(),
        Access::Unavailable => return unavailable(),
        Access::View => return control_answer(StatusCode::FORBIDDEN, ControlResult::NotAllowed),
        Access::Control => {},
    }
    if !s.limiter.allow(user, Instant::now()) {
        return control_answer(StatusCode::TOO_MANY_REQUESTS, ControlResult::TooMany);
    }
    if s.backend.plan(g).await != PlanView::Premium {
        return control_answer(StatusCode::FORBIDDEN, ControlResult::PremiumRequired);
    }
    match s.backend.control(user, g, req.into()).await {
        Ok(()) => {
            tracing::info!(guild = %g, user = %user, control = ?req, "dashboard control");
            s.hub.refresh(g).await;
            let view = s.backend.view(g).await;
            control_answer(StatusCode::OK, ControlResult::Done { view })
        },
        Err(ControlRefused::Conflict) => {
            let view = s.backend.view(g).await;
            control_answer(StatusCode::CONFLICT, ControlResult::Conflict { view })
        },
        Err(ControlRefused::GameInProgress) => {
            control_answer(StatusCode::LOCKED, ControlResult::GameInProgress)
        },
        Err(ControlRefused::NotPlaying) => {
            control_answer(StatusCode::CONFLICT, ControlResult::NotPlaying)
        },
        Err(ControlRefused::Failed) => {
            control_answer(StatusCode::INTERNAL_SERVER_ERROR, ControlResult::Failed)
        },
    }
}

/// The plan, or `Free` if it is not read within [`PRESENCE_TIMEOUT`]: the page
/// and a stream's first event are drawn regardless, and the stream's next
/// recheck corrects it.
pub(crate) async fn plan_or_free<B: Backend>(backend: &B, g: GuildId) -> PlanView {
    tokio::time::timeout(PRESENCE_TIMEOUT, backend.plan(g))
        .await
        .unwrap_or(PlanView::Free)
}

fn event(view: &QueueView, can_control: bool, plan: PlanView) -> Event {
    Event::default().data(
        serde_json::to_string(&PageState {
            view,
            can_control,
            plan,
        })
        .expect("the view serializes"),
    )
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
        let mut plan = plan_or_free(backend.as_ref(), g).await;
        let mut last = rx.borrow_and_update().clone();
        if tx.send(event(&last, can_control, plan)).await.is_err() {
            return;
        }
        let mut recheck = tokio::time::interval(RECHECK);
        recheck.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        recheck.tick().await;
        loop {
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    last = rx.borrow_and_update().clone();
                    if tx.send(event(&last, can_control, plan)).await.is_err() {
                        return;
                    }
                },
                _ = recheck.tick() => {
                    let (presence, now_plan) = tokio::join!(
                        tokio::time::timeout(PRESENCE_TIMEOUT, backend.presence(g, user)),
                        tokio::time::timeout(PRESENCE_TIMEOUT, backend.plan(g)),
                    );
                    let access = presence.map_or(Access::Unavailable, |p| decide(&p));
                    // Left the guild: close the stream.
                    if access == Access::Hidden {
                        return;
                    }
                    // Cannot tell right now: keep what we had.
                    let now_control = match access {
                        Access::Unavailable => can_control,
                        a => a == Access::Control,
                    };
                    let now_plan = now_plan.unwrap_or(plan);
                    if now_control != can_control || now_plan != plan {
                        can_control = now_control;
                        plan = now_plan;
                        if tx.send(event(&last, can_control, plan)).await.is_err() {
                            return;
                        }
                    }
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
    let (body, ctype): (&'static [u8], &'static str) = match name.as_str() {
        "app.js" => (
            include_bytes!("../assets/app.js"),
            "text/javascript; charset=utf-8",
        ),
        "find.js" => (
            include_bytes!("../assets/find.js"),
            "text/javascript; charset=utf-8",
        ),
        "sortable.min.js" => (
            include_bytes!("../assets/sortable.min.js"),
            "text/javascript; charset=utf-8",
        ),
        "history.js" => (
            include_bytes!("../assets/history.js"),
            "text/javascript; charset=utf-8",
        ),
        "settings.js" => (
            include_bytes!("../assets/settings.js"),
            "text/javascript; charset=utf-8",
        ),
        "app.css" => (
            include_bytes!("../assets/app.css"),
            "text/css; charset=utf-8",
        ),
        // The landing page's link preview (`og:image`), 1200x630. A copy of
        // cracktun.es's crack-tunes-og-image.webp.
        "og.webp" => (include_bytes!("../assets/og.webp"), "image/webp"),
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

/// Pages and answers drawn for one user: no cache, shared or back-button,
/// may keep them.
async fn no_store(mut resp: Response) -> Response {
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
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

/// `GET /health`'s body: fixed and specific to this origin, so a Cloudflare or
/// Caddy error page can never pass for the dashboard, and carrying the version
/// so a check can tell which build answered.
const HEALTH: &str = concat!("cracktunes dashboard ok ", env!("CARGO_PKG_VERSION"));

/// Unauthenticated and reads nothing: it says the web server is answering,
/// not that the bot is on the gateway, which the bot's own log reports.
async fn health() -> &'static str {
    HEALTH
}

/// The whole dashboard: pages, the stream, moves, assets, `/health`, and
/// catacombs at `/auth`. Every request has a timeout on its response; a
/// stream's body is not covered by it.
pub fn router<B: Backend>(state: WebState<B>) -> Router {
    let timeout = TimeoutLayer::with_status_code(StatusCode::SERVICE_UNAVAILABLE, REQUEST_TIMEOUT);
    let per_user = Router::new()
        .route("/", get(picker::<B>))
        .route("/g/{guild}", get(guild_page::<B>))
        .route("/g/{guild}/move", post(move_track::<B>))
        .route("/g/{guild}/control", post(control::<B>))
        .route("/g/{guild}/search", get(crate::find::search::<B>))
        .route("/g/{guild}/add", post(crate::find::add::<B>))
        .route("/g/{guild}/history", get(history_page::<B>))
        .route("/g/{guild}/history.json", get(history_json::<B>))
        .route("/g/{guild}/settings", get(settings_page::<B>))
        .route("/g/{guild}/settings/rooms", post(save_rooms::<B>))
        .route("/g/{guild}/settings/modlog", post(save_modlog::<B>))
        .route("/g/{guild}/settings/economy", post(save_economy::<B>))
        .route_layer(axum::middleware::map_response(no_store));
    let timed = Router::new()
        .merge(per_user)
        .route("/assets/{name}", get(asset))
        .route("/health", get(health))
        // The timeout bounds the opening reads only; tower-http never touches
        // the body, so the stream itself stays open.
        .route("/g/{guild}/events", get(events::<B>))
        .layer(timeout);
    Router::new()
        .merge(timed)
        .nest(
            "/auth",
            // The website flow only: /login, /callback, /logout. /callback
            // calls Discord; catacombs' client gives up at 15 s, and this
            // layer bounds the whole request.
            catacombs::router::<Arc<catacombs::Auth>>(catacombs::Flows::Web)
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

    /// #589: a signed-out `/` is a landing page with its own link preview.
    /// It used to redirect to the login, and Discord's link preview followed
    /// that on to Discord's OAuth page and showed Discord's app card instead.
    #[tokio::test]
    async fn signed_out_root_is_a_landing_page_with_its_own_preview() {
        let r = get(member_viewing(), "/", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers().get(header::LOCATION).is_none());
        let html = body(r).await;
        for want in [
            r#"<meta property="og:title" content="CrackTunes dashboard">"#,
            r#"<meta property="og:description" content="See what&#39;s playing in your Discord server and control the queue from your browser.">"#,
            r#"<meta property="og:url" content="https://dash.test/">"#,
            r#"<meta property="og:image" content="https://dash.test/assets/og.webp">"#,
            r#"<meta name="twitter:card" content="summary_large_image">"#,
            r#"<a class="button" href="/auth/login?return_to=%2F">Sign in with Discord</a>"#,
        ] {
            assert!(html.contains(want), "missing {want}\n{html}");
        }
    }

    #[tokio::test]
    async fn signed_out_guild_pages_are_sent_to_login_and_back() {
        let r = get(member_viewing(), "/g/5", None).await;
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            r.headers()[header::LOCATION],
            "/auth/login?return_to=%2Fg%2F5"
        );
    }

    /// The landing page's preview image (`og:image`), served from the binary.
    #[tokio::test]
    async fn the_preview_image_is_served_as_webp() {
        use http_body_util::BodyExt;
        let r = get(member_viewing(), "/assets/og.webp", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()[header::CONTENT_TYPE], "image/webp");
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
    }

    #[tokio::test]
    async fn health_answers_signed_out_with_this_builds_version() {
        let r = get(member_viewing(), "/health", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/plain"));
        assert_eq!(
            body(r).await,
            concat!("cracktunes dashboard ok ", env!("CARGO_PKG_VERSION"))
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

    #[tokio::test]
    async fn the_page_and_the_first_event_carry_the_plan() {
        use crate::view::PlanView;
        #[derive(serde::Deserialize)]
        struct State {
            plan: PlanView,
        }
        for plan in [PlanView::Free, PlanView::Premium] {
            let fake = controller();
            *fake.plan.lock().unwrap() = plan;
            let html = body(get(fake.clone(), "/g/5", Some(&session(9))).await).await;
            let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
            let end = start + html[start..].find("</script>").unwrap();
            let page: State = serde_json::from_str(&html[start..end]).unwrap();
            assert_eq!(page.plan, plan, "page");
            let r = get(fake, "/g/5/events", Some(&session(9))).await;
            let first: State = serde_json::from_str(&first_event(r).await).unwrap();
            assert_eq!(first.plan, plan, "first event");
        }
    }

    /// The page and the first event are drawn even if the plan never comes:
    /// as free, and within the bound. The outer timeout turns a hang into a failure.
    #[tokio::test(start_paused = true)]
    async fn a_hung_plan_read_draws_the_page_and_first_event_as_free() {
        use crate::view::PlanView;
        use std::sync::atomic::Ordering::SeqCst;
        #[derive(serde::Deserialize)]
        struct State {
            plan: PlanView,
        }
        let fake = controller();
        fake.plan_hangs.store(true, SeqCst);
        let bound = crate::routes::PRESENCE_TIMEOUT + std::time::Duration::from_secs(1);
        let r = tokio::time::timeout(bound, get(fake.clone(), "/g/5", Some(&session(9))))
            .await
            .expect("the page waited on the plan");
        assert_eq!(r.status(), StatusCode::OK);
        let html = body(r).await;
        let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
        let end = start + html[start..].find("</script>").unwrap();
        let page: State = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(page.plan, PlanView::Free, "page");
        let first = tokio::time::timeout(bound, async {
            let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
            next_data(&mut r.into_body()).await
        })
        .await
        .expect("the first event waited on the plan");
        let first: State = serde_json::from_str(&first).unwrap();
        assert_eq!(first.plan, PlanView::Free, "first event");
    }

    #[tokio::test(start_paused = true)]
    async fn a_premium_grant_reaches_an_open_stream_at_the_next_recheck() {
        use crate::view::PlanView;
        use http_body_util::BodyExt;
        #[derive(serde::Deserialize)]
        struct State {
            plan: PlanView,
            can_control: bool,
        }
        let fake = controller();
        *fake.plan.lock().unwrap() = PlanView::Free;
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let first = std::str::from_utf8(&first).unwrap();
        let first = first
            .lines()
            .find_map(|l| l.strip_prefix("data: "))
            .unwrap();
        let first: State = serde_json::from_str(first).unwrap();
        assert_eq!(first.plan, PlanView::Free);
        *fake.plan.lock().unwrap() = PlanView::Premium;
        tokio::time::sleep(RECHECK + std::time::Duration::from_secs(1)).await;
        let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        let data = text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        let s: State = serde_json::from_str(data).unwrap();
        assert_eq!(s.plan, PlanView::Premium);
        assert!(s.can_control);
    }

    #[test]
    fn the_csp_allows_nothing_unsafe() {
        assert!(!crate::routes::CSP.contains("unsafe-"));
    }

    #[test]
    fn the_csp_forbids_string_to_html_sinks() {
        let csp = crate::routes::CSP;
        assert!(csp.contains("require-trusted-types-for 'script'"));
        assert!(csp.contains("trusted-types 'none'"));
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
    async fn auth_mounts_the_website_flow_only() {
        let r = get(member_viewing(), "/auth/login?return_to=%2F", None).await;
        assert!(r.status().is_redirection(), "{}", r.status());
        assert!(r.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .starts_with("https://discord.com/"));
        // The dashboard never posts a code, so the Activity flow's routes are
        // not mounted at all.
        for uri in ["/auth/exchange", "/auth/refresh", "/auth/revoke"] {
            let r = app(member_viewing())
                .oneshot(Request::post(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::NOT_FOUND, "{uri}");
        }
    }

    #[tokio::test]
    async fn find_js_is_served_as_javascript() {
        let r = get(member_viewing(), "/assets/find.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            r.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
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
    use serenity::all::GuildId;
    use uuid::Uuid;

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

    /// `playing()` after track 3 moved to the front of what is up next.
    fn reordered() -> QueueView {
        let QueueView::Playing { now, upcoming, .. } = playing() else {
            unreachable!()
        };
        QueueView::Playing {
            now,
            upcoming: upcoming.into_iter().rev().collect(),
            rev: 8,
            paused: false,
            looping: false,
        }
    }

    fn move_request() -> Request<Body> {
        Request::post("/g/5/move")
            .header(header::COOKIE, session(9))
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ORIGIN, ORIGIN)
            .body(Body::from(MOVE))
            .unwrap()
    }

    #[tokio::test]
    async fn a_dashboard_move_is_recorded_as_the_mover() {
        let fake = controller();
        let r = crate::routes::router(state(fake.clone()))
            .oneshot(move_request())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(fake.last_mover(), Some(serenity::all::UserId::new(9)));
    }

    const SKIP: &str = r#"{"type":"skip","id":"00000000-0000-0000-0000-000000000001"}"#;

    fn control_request(
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        text: &str,
    ) -> Request<Body> {
        let mut req = Request::post("/g/5/control");
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

    async fn post_control(
        fake: std::sync::Arc<FakeBackend>,
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        text: &str,
    ) -> axum::response::Response {
        app(fake)
            .oneshot(control_request(cookie, ctype, origin, text))
            .await
            .unwrap()
    }

    async fn post_skip(fake: std::sync::Arc<FakeBackend>) -> axum::response::Response {
        post_control(
            fake,
            Some(&session(9)),
            Some("application/json"),
            Some(ORIGIN),
            SKIP,
        )
        .await
    }

    #[tokio::test]
    async fn a_premium_controller_skips_and_the_backend_gets_exactly_that() {
        use crate::backend::Control;
        let fake = controller();
        let r = post_skip(fake.clone()).await;
        assert_eq!(r.status(), StatusCode::OK);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "done");
        assert_eq!(
            *fake.controls.lock().unwrap(),
            vec![(
                GuildId::new(5),
                serenity::all::UserId::new(9),
                Control::Skip {
                    expect: Uuid::from_u128(1)
                }
            )]
        );
    }

    #[tokio::test]
    async fn control_refusals_before_the_backend_control_nothing() {
        type Case = (
            Option<String>,
            Option<&'static str>,
            Option<&'static str>,
            &'static str,
            Membership,
            StatusCode,
        );
        let json = Some("application/json");
        let cases: Vec<Case> = vec![
            (
                None,
                json,
                Some(ORIGIN),
                SKIP,
                Membership::Member,
                StatusCode::UNAUTHORIZED,
            ),
            (
                Some(session(9)),
                Some("text/plain"),
                Some(ORIGIN),
                SKIP,
                Membership::Member,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                Some(session(9)),
                None,
                Some(ORIGIN),
                SKIP,
                Membership::Member,
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                Some(session(9)),
                json,
                Some("https://evil.example"),
                SKIP,
                Membership::Member,
                StatusCode::FORBIDDEN,
            ),
            (
                Some(session(9)),
                json,
                None,
                SKIP,
                Membership::Member,
                StatusCode::FORBIDDEN,
            ),
            (
                Some(session(9)),
                json,
                Some(ORIGIN),
                r#"{"type":"stop"}"#,
                Membership::Member,
                StatusCode::BAD_REQUEST,
            ),
            (
                Some(session(9)),
                json,
                Some(ORIGIN),
                "{not json",
                Membership::Member,
                StatusCode::BAD_REQUEST,
            ),
            (
                Some(session(9)),
                json,
                Some(ORIGIN),
                SKIP,
                Membership::NotMember,
                StatusCode::NOT_FOUND,
            ),
            (
                Some(session(9)),
                json,
                Some(ORIGIN),
                SKIP,
                Membership::Unknown,
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ];
        for (cookie, ctype, origin, text, membership, want) in cases {
            let fake = FakeBackend::new(membership, Some(BOT_CHANNEL), playing());
            let r = post_control(fake.clone(), cookie.as_deref(), ctype, origin, text).await;
            assert_eq!(
                r.status(),
                want,
                "{ctype:?} {origin:?} {text} {membership:?}"
            );
            assert_eq!(fake.control_count(), 0, "nothing ran for {want}");
        }
    }

    #[tokio::test]
    async fn a_member_not_in_voice_is_not_allowed_to_control() {
        for channel in [None, Some(serenity::all::ChannelId::new(1))] {
            let fake = FakeBackend::new(Membership::Member, channel, playing());
            let r = post_skip(fake.clone()).await;
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
            let a: Answer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, "not_allowed");
            assert_eq!(fake.control_count(), 0);
        }
    }

    #[tokio::test]
    async fn a_free_server_is_premium_required_and_never_reaches_the_backend() {
        let fake = controller();
        *fake.plan.lock().unwrap() = crate::view::PlanView::Free;
        let r = post_skip(fake.clone()).await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "premium_required");
        assert_eq!(fake.control_count(), 0);
    }

    /// 🔑 The plan is read on every request, never kept: a revoke applies to
    /// the very next control.
    #[tokio::test]
    async fn every_control_reads_the_plan_afresh() {
        use std::sync::atomic::Ordering::SeqCst;
        let fake = controller();
        let router = app(fake.clone()); // one router, so one state
        let send = || {
            router.clone().oneshot(control_request(
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                SKIP,
            ))
        };
        for i in 0..2 {
            assert_eq!(
                send().await.unwrap().status(),
                StatusCode::OK,
                "control {i}"
            );
        }
        assert_eq!(fake.plan_calls.load(SeqCst), 2);
        *fake.plan.lock().unwrap() = crate::view::PlanView::Free;
        let r = send().await.unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "premium_required");
        assert_eq!(fake.plan_calls.load(SeqCst), 3);
        assert_eq!(fake.control_count(), 2);
    }

    #[tokio::test]
    async fn the_sixth_control_in_ten_seconds_is_too_many_and_runs_nothing() {
        let fake = controller();
        let router = app(fake.clone()); // one router, so one limiter
        for i in 0..5 {
            let r = router
                .clone()
                .oneshot(control_request(
                    Some(&session(9)),
                    Some("application/json"),
                    Some(ORIGIN),
                    SKIP,
                ))
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::OK, "control {i}");
        }
        let r = router
            .clone()
            .oneshot(control_request(
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                SKIP,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "too_many");
        assert_eq!(fake.control_count(), 5);
        // Another user is not held up by it.
        let r = router
            .oneshot(control_request(
                Some(&session(10)),
                Some("application/json"),
                Some(ORIGIN),
                SKIP,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_stale_skip_is_a_conflict_with_the_fresh_view() {
        use crate::backend::ControlRefused;
        let fake = controller();
        *fake.control_result.lock().unwrap() = Err(ControlRefused::Conflict);
        let r = post_skip(fake).await;
        assert_eq!(r.status(), StatusCode::CONFLICT);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "conflict");
        assert_eq!(a.view, Some(playing()));
    }

    #[tokio::test]
    async fn control_refusals_map_to_their_answers() {
        use crate::backend::ControlRefused;
        let cases = [
            (
                ControlRefused::GameInProgress,
                StatusCode::LOCKED,
                "game_in_progress",
            ),
            (
                ControlRefused::NotPlaying,
                StatusCode::CONFLICT,
                "not_playing",
            ),
            (
                ControlRefused::Failed,
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed",
            ),
        ];
        for (refusal, status, result) in cases {
            let fake = controller();
            *fake.control_result.lock().unwrap() = Err(refusal);
            let r = post_skip(fake).await;
            assert_eq!(r.status(), status, "{refusal:?}");
            let a: Answer = serde_json::from_str(&body(r).await).unwrap();
            assert_eq!(a.result, result);
            assert!(a.view.is_none(), "{refusal:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_control_reaches_open_watchers_at_once() {
        use crate::watch::TICK;
        let fake = controller();
        *fake.view_after_control.lock().unwrap() = Some(reordered());
        let st = state(fake.clone());
        let mut rx = st.hub.subscribe(GuildId::new(5)).await;
        rx.borrow_and_update();
        let r = crate::routes::router(st)
            .oneshot(control_request(
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                SKIP,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(
            a.view,
            Some(reordered()),
            "the answer is read after the control"
        );
        tokio::time::timeout(TICK / 2, rx.changed())
            .await
            .expect("published without waiting for a tick")
            .unwrap();
        assert_eq!(**rx.borrow(), reordered(), "watchers see the new queue");
    }

    #[tokio::test]
    async fn a_move_is_not_rate_limited() {
        let fake = controller();
        let router = app(fake.clone());
        for i in 0..6 {
            let r = router.clone().oneshot(move_request()).await.unwrap();
            assert_eq!(r.status(), StatusCode::OK, "move {i}");
        }
        assert_eq!(fake.move_count(), 6);
    }

    #[tokio::test(start_paused = true)]
    async fn a_move_reaches_open_watchers_at_once() {
        use crate::watch::TICK;
        let fake = controller();
        *fake.view_after_move.lock().unwrap() = Some(reordered());
        let st = state(fake.clone());
        let mut rx = st.hub.subscribe(GuildId::new(5)).await;
        rx.borrow_and_update();
        let r = crate::routes::router(st)
            .oneshot(move_request())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(
            a.view,
            Some(reordered()),
            "the answer is read after the move"
        );
        // Half a tick: the poller's own tick must not be what delivers it.
        tokio::time::timeout(TICK / 2, rx.changed())
            .await
            .expect("published without waiting for a tick")
            .unwrap();
        assert_eq!(**rx.borrow(), reordered(), "watchers see the moved queue");
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_move_publishes_nothing_and_answers_with_the_queue_as_it_is() {
        use crate::watch::TICK;
        use std::sync::atomic::Ordering::SeqCst;
        let fake = controller();
        *fake.move_result.lock().unwrap() = Err(MoveRefused::Absent);
        let st = state(fake.clone());
        let mut rx = st.hub.subscribe(GuildId::new(5)).await;
        rx.borrow_and_update();
        // The queue changed since the hub's last read; its next tick is not due.
        *fake.view.lock().unwrap() = reordered();
        let reads = fake.view_calls.load(SeqCst);
        let r = crate::routes::router(st)
            .oneshot(move_request())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::CONFLICT);
        let a: Answer = serde_json::from_str(&body(r).await).unwrap();
        assert_eq!(a.result, "conflict");
        assert_eq!(
            a.view,
            Some(reordered()),
            "a fresh read, not the hub's last"
        );
        tokio::time::sleep(TICK / 2).await;
        assert_eq!(
            fake.view_calls.load(SeqCst),
            reads + 1,
            "only the answer's read"
        );
        assert!(!rx.has_changed().unwrap(), "nothing published");
    }

    #[tokio::test]
    async fn per_user_answers_are_never_stored_and_assets_still_are() {
        for uri in ["/", "/g/5"] {
            let r = get(member_viewing(), uri, Some(&session(9))).await;
            assert_eq!(r.status(), StatusCode::OK, "{uri}");
            assert_eq!(r.headers()[header::CACHE_CONTROL], "no-store", "{uri}");
        }
        let r = post_move(
            controller(),
            Some(&session(9)),
            Some("application/json"),
            Some(ORIGIN),
            MOVE,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()[header::CACHE_CONTROL], "no-store");
        let r = get(member_viewing(), "/assets/app.js", None).await;
        assert_eq!(r.headers()[header::CACHE_CONTROL], "no-cache");
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
        // Bounded: a stream that never ends would otherwise spin on the paused clock.
        let drained = tokio::time::timeout(std::time::Duration::from_secs(60), async {
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
        })
        .await;
        assert!(drained.is_ok(), "the stream ended");
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

    /// The next `data:` payload, skipping keep-alive comments.
    async fn next_data(body: &mut Body) -> String {
        use http_body_util::BodyExt;
        tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                let frame = body.frame().await.expect("stream open").unwrap();
                if let Ok(data) = frame.into_data() {
                    let text = std::str::from_utf8(&data).unwrap().to_owned();
                    if let Some(d) = text.lines().find_map(|l| l.strip_prefix("data: ")) {
                        return d.to_owned();
                    }
                }
            }
        })
        .await
        .expect("a data event")
    }

    #[derive(serde::Deserialize)]
    struct Pushed {
        view: QueueView,
    }

    #[tokio::test(start_paused = true)]
    async fn a_queue_change_reaches_an_open_stream() {
        let fake = controller();
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let first: Pushed = serde_json::from_str(&next_data(&mut body).await).unwrap();
        assert_eq!(first.view, playing());
        *fake.view.lock().unwrap() = QueueView::Idle;
        let next: Pushed = serde_json::from_str(&next_data(&mut body).await).unwrap();
        assert_eq!(next.view, QueueView::Idle);
    }

    #[tokio::test(start_paused = true)]
    async fn a_closed_stream_stops_reading_the_backend() {
        use std::sync::atomic::Ordering::SeqCst;
        let fake = controller();
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let _ = next_data(&mut body).await;
        drop(body);
        let quiet = RECHECK + crate::watch::LINGER + std::time::Duration::from_secs(10);
        tokio::time::sleep(quiet).await;
        let (p, v) = (
            fake.presence_calls.load(SeqCst),
            fake.view_calls.load(SeqCst),
        );
        tokio::time::sleep(quiet).await;
        assert_eq!(
            fake.presence_calls.load(SeqCst),
            p,
            "no more presence reads"
        );
        assert_eq!(fake.view_calls.load(SeqCst), v, "no more view reads");
    }

    #[tokio::test(start_paused = true)]
    async fn a_hung_presence_recheck_does_not_stall_the_stream() {
        use std::sync::atomic::Ordering::SeqCst;
        let fake = controller();
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        let mut body = r.into_body();
        let _ = next_data(&mut body).await;
        fake.presence_hangs.store(true, SeqCst);
        tokio::time::sleep(RECHECK + std::time::Duration::from_secs(1)).await; // recheck now hangs
        *fake.view.lock().unwrap() = QueueView::Idle;
        let next: Pushed = serde_json::from_str(&next_data(&mut body).await).unwrap();
        assert_eq!(next.view, QueueView::Idle);
    }

    #[tokio::test]
    async fn an_unknown_membership_is_503_for_the_stream_and_the_move() {
        let fake = FakeBackend::new(Membership::Unknown, Some(BOT_CHANNEL), playing());
        let r = get(fake.clone(), "/g/5/events", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        let r = post_move(
            fake.clone(),
            Some(&session(9)),
            Some("application/json"),
            Some(ORIGIN),
            MOVE,
        )
        .await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(fake.move_count(), 0);
    }

    use crate::{
        access::HistoryAccess,
        backend::HistoryError,
        history::{HistoryPage, HistoryRow, Who},
    };

    fn history_fake(access: HistoryAccess) -> std::sync::Arc<FakeBackend> {
        let fake = member_viewing();
        *fake.history_access.lock().unwrap() = access;
        fake
    }

    #[tokio::test]
    async fn signed_out_history_goes_to_login_and_its_json_is_401() {
        let r = get(member_viewing(), "/g/5/history", None).await;
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            r.headers()[header::LOCATION],
            "/auth/login?return_to=%2Fg%2F5%2Fhistory"
        );
        let r = get(member_viewing(), "/g/5/history.json", None).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn history_access_maps_to_its_answers() {
        for (access, status) in [
            (HistoryAccess::Hidden, StatusCode::NOT_FOUND),
            (HistoryAccess::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
            (HistoryAccess::Forbidden, StatusCode::FORBIDDEN),
            (HistoryAccess::Allowed, StatusCode::OK),
        ] {
            for uri in ["/g/5/history", "/g/5/history.json"] {
                let r = get(history_fake(access), uri, Some(&session(9))).await;
                assert_eq!(r.status(), status, "{access:?} {uri}");
            }
        }
    }

    #[tokio::test]
    async fn bad_history_queries_are_400_and_never_reach_the_backend() {
        for q in [
            "user=abc",
            "action=dance",
            "source=email",
            "since=3y",
            "before=x",
            "before=1&after=2",
        ] {
            let fake = history_fake(HistoryAccess::Allowed);
            let r = get(
                fake.clone(),
                &format!("/g/5/history.json?{q}"),
                Some(&session(9)),
            )
            .await;
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{q}");
            assert!(body(r).await.contains("\"error\""), "{q}");
            assert!(fake.history_queries.lock().unwrap().is_empty(), "{q}");
        }
    }

    #[tokio::test]
    async fn the_backend_gets_exactly_the_query_sent() {
        let fake = history_fake(HistoryAccess::Allowed);
        let r = get(
            fake.clone(),
            "/g/5/history.json?user=7&action=move&source=web&since=6h&after=40",
            Some(&session(9)),
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        let got = fake.history_queries.lock().unwrap().clone();
        assert_eq!(
            got,
            vec![crate::history::parse_history_query(Some(
                "user=7&action=move&source=web&since=6h&after=40"
            ))
            .unwrap()]
        );
    }

    #[tokio::test]
    async fn history_json_carries_the_backends_page() {
        let fake = history_fake(HistoryAccess::Allowed);
        let page = HistoryPage {
            rows: vec![HistoryRow {
                id: 3,
                at: "2026-10-02T12:00:00+00:00".into(),
                who: Who::Bot,
                how: "idle timeout".into(),
                what: "left voice, 2 tracks discarded".into(),
            }],
            older: true,
            game_hidden: true,
            capped: true,
        };
        *fake.history_result.lock().unwrap() = Ok(page.clone());
        let r = get(fake, "/g/5/history.json", Some(&session(9))).await;
        let text = body(r).await;
        assert!(text.contains("\"capped\":true"), "{text}");
        let got: HistoryPage = serde_json::from_str(&text).unwrap();
        assert_eq!(got, page);
    }

    #[tokio::test]
    async fn a_missing_database_is_503_with_its_reason() {
        let fake = history_fake(HistoryAccess::Allowed);
        *fake.history_result.lock().unwrap() = Err(HistoryError::NoDatabase);
        let r = get(fake.clone(), "/g/5/history.json", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(body(r).await.contains(crate::history::NO_DATABASE));
        let r = get(fake, "/g/5/history", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn the_history_page_inlines_the_first_page() {
        let fake = history_fake(HistoryAccess::Allowed);
        let page = HistoryPage {
            rows: vec![HistoryRow {
                id: 1,
                at: "2026-10-02T12:00:00+00:00".into(),
                who: Who::Member {
                    id: "7".into(),
                    name: Some("</script><img src=x>".into()),
                },
                how: "/play".into(),
                what: "added </script>".into(),
            }],
            older: false,
            game_hidden: false,
            capped: false,
        };
        *fake.history_result.lock().unwrap() = Ok(page.clone());
        let html = body(get(fake, "/g/5/history", Some(&session(9))).await).await;
        assert!(!html.contains("<img"), "{html}");
        let start = html.find("id=\"initial\">").unwrap() + "id=\"initial\">".len();
        let end = start + html[start..].find("</script>").unwrap();
        let got: HistoryPage = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(got, page);
        assert!(html.contains("src=\"/assets/history.js\""));
    }

    #[tokio::test]
    async fn the_queue_page_links_history_for_managers_only() {
        let html = body(
            get(
                history_fake(HistoryAccess::Allowed),
                "/g/5",
                Some(&session(9)),
            )
            .await,
        )
        .await;
        assert!(html.contains("href=\"/g/5/history\""));
        for access in [HistoryAccess::Forbidden, HistoryAccess::Unavailable] {
            let html = body(get(history_fake(access), "/g/5", Some(&session(9))).await).await;
            assert!(!html.contains("/history"), "{access:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_stalled_history_check_leaves_the_queue_page_without_the_link() {
        use std::sync::atomic::Ordering::SeqCst;
        let fake = history_fake(HistoryAccess::Allowed);
        fake.history_access_hangs.store(true, SeqCst);
        // Well under the request timeout, so only the link's own bound can
        // answer in time.
        let r = tokio::time::timeout(
            super::HISTORY_LINK_TIMEOUT + std::time::Duration::from_secs(1),
            get(fake, "/g/5", Some(&session(9))),
        )
        .await
        .expect("the queue page waited on the history check");
        assert_eq!(r.status(), StatusCode::OK);
        let html = body(r).await;
        assert!(html.contains("id=\"initial\""), "the queue page itself");
        assert!(!html.contains("/history"), "{html}");
    }

    #[tokio::test]
    async fn a_failed_history_read_is_503_with_its_reason() {
        let fake = history_fake(HistoryAccess::Allowed);
        *fake.history_result.lock().unwrap() = Err(HistoryError::Failed);
        let r = get(fake.clone(), "/g/5/history.json", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        let got = body(r).await;
        assert!(got.contains(crate::history::HISTORY_FAILED), "{got}");
        let r = get(fake, "/g/5/history", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn history_js_is_served() {
        let r = get(member_viewing(), "/assets/history.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/javascript"));
    }

    // ---- rooms settings ----

    use crate::settings::{ChannelOption, RoomSettingsForm, SaveRefused};
    use crack_core::{
        messaging::messages::*,
        rooms::settings::{render_room_name, SettingsError},
    };

    fn settings_fake(access: HistoryAccess) -> std::sync::Arc<FakeBackend> {
        history_fake(access)
    }

    fn post_settings_req(
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        text: &str,
    ) -> Request<Body> {
        let mut req = Request::post("/g/5/settings/rooms");
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

    async fn post_settings(
        fake: std::sync::Arc<FakeBackend>,
        text: &str,
    ) -> axum::response::Response {
        app(fake)
            .oneshot(post_settings_req(
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                text,
            ))
            .await
            .unwrap()
    }

    const FORM: &str = r#"{"enabled":true,"hub":"123","name_template":"{name}'s den","default_limit":4,"empty_grace_secs":60}"#;

    #[tokio::test]
    async fn settings_page_404s_for_non_members_403s_for_non_managers() {
        let r = get(member_viewing(), "/g/5/settings", None).await;
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            r.headers()[header::LOCATION],
            "/auth/login?return_to=%2Fg%2F5%2Fsettings"
        );
        for (access, status) in [
            (HistoryAccess::Hidden, StatusCode::NOT_FOUND),
            (HistoryAccess::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
            (HistoryAccess::Forbidden, StatusCode::FORBIDDEN),
            (HistoryAccess::Allowed, StatusCode::OK),
        ] {
            let r = get(settings_fake(access), "/g/5/settings", Some(&session(9))).await;
            assert_eq!(r.status(), status, "{access:?}");
        }
        let r = get(
            settings_fake(HistoryAccess::Forbidden),
            "/g/5/settings",
            Some(&session(9)),
        )
        .await;
        assert!(body(r)
            .await
            .contains(&crate::page::esc(DASH_SETTINGS_NEEDS_MANAGE)));
    }

    #[tokio::test]
    async fn settings_page_lists_voice_channels_and_current_values() {
        let fake = settings_fake(HistoryAccess::Allowed);
        *fake.room_view.lock().unwrap() = crate::settings::RoomSettingsView {
            enabled: true,
            hub: Some("22".into()),
            name_template: "<b>{name}</b> \"x\"".into(),
            default_limit: 4,
            empty_grace_secs: 60,
            voice_channels: vec![
                ChannelOption {
                    id: "11".into(),
                    name: "Music".into(),
                },
                ChannelOption {
                    id: "22".into(),
                    name: "Join <i>here</i>".into(),
                },
            ],
            findings: Some(vec![
                "Give CrackTunes Move Members on Games".into(),
                "<script>x</script>".into(),
            ]),
        };
        let r = get(fake, "/g/5/settings", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let html = body(r).await;
        for needle in [
            "<option value=\"11\">Music</option>",
            "<option value=\"22\" selected>Join &lt;i&gt;here&lt;/i&gt;</option>",
            "id=\"s-enabled\" checked",
            "value=\"&lt;b&gt;{name}&lt;/b&gt; &quot;x&quot;\"",
            "id=\"s-limit\"",
            "value=\"4\"",
            "<option value=\"60\" selected>",
            "<option value=\"0\">",
            "<option value=\"300\">",
            "<li>Give CrackTunes Move Members on Games</li>",
            "<li>&lt;script&gt;x&lt;/script&gt;</li>",
            "https://github.com/cycle-five/cracktunes/blob/master/docs/rooms-and-mod-log.md",
            "<script src=\"/assets/settings.js\"></script>",
        ] {
            assert!(html.contains(needle), "missing {needle}: {html}");
        }
        assert!(!html.contains("<script>x"), "{html}");
        // The first preview is what the bot would name the room.
        let preview = render_room_name("<b>{name}</b> \"x\"", "Saucey");
        assert!(
            html.contains(&format!(
                "<output id=\"s-preview\">{}</output>",
                crate::page::esc(&preview)
            )),
            "{html}"
        );
    }

    #[tokio::test]
    async fn settings_page_says_why_there_are_no_findings() {
        let fake = settings_fake(HistoryAccess::Allowed);
        let html = body(get(fake.clone(), "/g/5/settings", Some(&session(9))).await).await;
        assert!(html.contains(ROOM_STATUS_NO_HUB_YET), "{html}");
        fake.room_view.lock().unwrap().hub = Some("22".into());
        let html = body(get(fake.clone(), "/g/5/settings", Some(&session(9))).await).await;
        assert!(
            html.contains(&crate::page::esc(ROOM_STATUS_UNKNOWN)),
            "{html}"
        );
        fake.room_view.lock().unwrap().findings = Some(vec![]);
        let html = body(get(fake, "/g/5/settings", Some(&session(9))).await).await;
        assert!(html.contains(ROOM_STATUS_READY), "{html}");
    }

    #[tokio::test]
    async fn posting_settings_requires_json() {
        let fake = settings_fake(HistoryAccess::Allowed);
        for ctype in [
            None,
            Some("text/plain"),
            Some("application/x-www-form-urlencoded"),
        ] {
            let r = app(fake.clone())
                .oneshot(post_settings_req(
                    Some(&session(9)),
                    ctype,
                    Some(ORIGIN),
                    FORM,
                ))
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE, "{ctype:?}");
        }
        let r = app(fake.clone())
            .oneshot(post_settings_req(
                Some(&session(9)),
                Some("application/json"),
                Some("https://evil.test"),
                FORM,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let r = app(fake.clone())
            .oneshot(post_settings_req(
                None,
                Some("application/json"),
                Some(ORIGIN),
                FORM,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        assert!(fake.saves.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_managers_may_post_settings() {
        for (access, status) in [
            (HistoryAccess::Hidden, StatusCode::NOT_FOUND),
            (HistoryAccess::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
            (HistoryAccess::Forbidden, StatusCode::FORBIDDEN),
        ] {
            let fake = settings_fake(access);
            let r = post_settings(fake.clone(), FORM).await;
            assert_eq!(r.status(), status, "{access:?}");
            assert!(fake.saves.lock().unwrap().is_empty(), "{access:?}");
        }
    }

    #[tokio::test]
    async fn a_manager_saves_and_the_backend_gets_exactly_that_form() {
        let fake = settings_fake(HistoryAccess::Allowed);
        let r = post_settings(fake.clone(), FORM).await;
        assert_eq!(r.status(), StatusCode::OK);
        let got = body(r).await;
        assert!(got.contains("\"name_template\""), "{got}");
        assert_eq!(
            fake.saves.lock().unwrap().clone(),
            vec![(
                GuildId::new(5),
                serenity::all::UserId::new(9),
                RoomSettingsForm {
                    enabled: true,
                    hub: Some("123".into()),
                    name_template: "{name}'s den".into(),
                    default_limit: 4,
                    empty_grace_secs: 60,
                }
            )]
        );
        // A hub-less, disabled form is valid too.
        let r = post_settings(
            fake.clone(),
            r#"{"enabled":false,"hub":null,"name_template":"x","default_limit":0,"empty_grace_secs":0}"#,
        )
        .await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(fake.saves.lock().unwrap()[1].2.hub, None);
    }

    #[tokio::test]
    async fn invalid_settings_are_400_with_the_reason() {
        for (refused, status, text) in [
            (
                SaveRefused::Invalid(SettingsError::NoDatabase),
                StatusCode::BAD_REQUEST,
                DASH_SETTINGS_NO_DATABASE,
            ),
            (
                SaveRefused::Invalid(SettingsError::NoHub),
                StatusCode::BAD_REQUEST,
                DASH_SETTINGS_NO_HUB,
            ),
            (
                SaveRefused::Invalid(SettingsError::BadTemplate),
                StatusCode::BAD_REQUEST,
                DASH_SETTINGS_BAD_TEMPLATE,
            ),
            (
                SaveRefused::Invalid(SettingsError::BadLimit),
                StatusCode::BAD_REQUEST,
                DASH_SETTINGS_BAD_LIMIT,
            ),
            (
                SaveRefused::Invalid(SettingsError::BadGrace),
                StatusCode::BAD_REQUEST,
                DASH_SETTINGS_BAD_GRACE,
            ),
            (
                SaveRefused::UnknownChannel,
                StatusCode::BAD_REQUEST,
                DASH_SETTINGS_UNKNOWN_CHANNEL,
            ),
            (
                SaveRefused::Failed,
                StatusCode::SERVICE_UNAVAILABLE,
                DASH_SETTINGS_FAILED,
            ),
            (
                SaveRefused::NotLoaded,
                StatusCode::SERVICE_UNAVAILABLE,
                DASH_SETTINGS_NOT_LOADED,
            ),
        ] {
            let fake = settings_fake(HistoryAccess::Allowed);
            *fake.save_result.lock().unwrap() = Err(refused);
            let r = post_settings(fake, FORM).await;
            assert_eq!(r.status(), status, "{refused:?}");
            let got = body(r).await;
            assert!(
                got.contains(&format!(
                    "\"error\":{}",
                    serde_json::to_string(text).unwrap()
                )),
                "{refused:?}: {got}"
            );
        }
        // Unreadable bodies never reach the backend.
        let fake = settings_fake(HistoryAccess::Allowed);
        for bad in [
            "{}",
            "nope",
            r#"{"enabled":true,"hub":null,"name_template":"x","default_limit":300,"empty_grace_secs":30}"#,
        ] {
            let r = post_settings(fake.clone(), bad).await;
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert!(fake.saves.lock().unwrap().is_empty());
    }

    // ---- mod log settings ----

    use crate::settings::{ModLogSaveRefused, ModLogSettingsForm, ModLogSettingsView};
    use crack_core::modlog::settings::ModLogSettingsError;

    fn post_modlog_req(
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        text: &str,
    ) -> Request<Body> {
        let mut req = Request::post("/g/5/settings/modlog");
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

    async fn post_modlog(
        fake: std::sync::Arc<FakeBackend>,
        text: &str,
    ) -> axum::response::Response {
        app(fake)
            .oneshot(post_modlog_req(
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                text,
            ))
            .await
            .unwrap()
    }

    const LOG_FORM: &str = r#"{"enabled":true,"channel":"123"}"#;

    #[tokio::test]
    async fn modlog_section_lists_text_channels_and_current_values() {
        let fake = settings_fake(HistoryAccess::Allowed);
        *fake.modlog_view.lock().unwrap() = ModLogSettingsView {
            enabled: true,
            channel: Some("22".into()),
            text_channels: vec![
                ChannelOption {
                    id: "11".into(),
                    name: "general".into(),
                },
                ChannelOption {
                    id: "22".into(),
                    name: "mod <i>log</i>".into(),
                },
            ],
            findings: Some(vec![
                "Give CrackTunes View Audit Log in Server Settings → Roles".into(),
                "<script>x</script>".into(),
            ]),
            visibility: Some("Anyone who can see #mod <i>log</i> can read the log.".into()),
        };
        let r = get(fake.clone(), "/g/5/settings", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let html = body(r).await;
        for needle in [
            "id=\"modlog-form\"",
            "id=\"m-enabled\" checked",
            "<option value=\"11\">general</option>",
            "<option value=\"22\" selected>mod &lt;i&gt;log&lt;/i&gt;</option>",
            "<p id=\"m-visible\">Anyone who can see #mod &lt;i&gt;log&lt;/i&gt; can read the log.</p>",
            "<li>Give CrackTunes View Audit Log in Server Settings → Roles</li>",
            "<li>&lt;script&gt;x&lt;/script&gt;</li>",
            "https://github.com/cycle-five/cracktunes/blob/master/docs/rooms-and-mod-log.md#mod-log",
        ] {
            assert!(html.contains(needle), "missing {needle}: {html}");
        }
        assert!(!html.contains("<script>x"), "{html}");
        // No channel yet, then unknown, then ready: each says why.
        *fake.modlog_view.lock().unwrap() = ModLogSettingsView {
            enabled: false,
            channel: None,
            text_channels: vec![],
            findings: None,
            visibility: None,
        };
        let html = body(get(fake.clone(), "/g/5/settings", Some(&session(9))).await).await;
        assert!(
            html.contains(&crate::page::esc(DASH_MODLOG_NO_CHANNEL_YET)),
            "{html}"
        );
        assert!(!html.contains("id=\"m-enabled\" checked"), "{html}");
        fake.modlog_view.lock().unwrap().channel = Some("22".into());
        let html = body(get(fake.clone(), "/g/5/settings", Some(&session(9))).await).await;
        assert!(
            html.contains(&crate::page::esc(MODLOG_STATUS_UNKNOWN)),
            "{html}"
        );
        fake.modlog_view.lock().unwrap().findings = Some(vec![]);
        let html = body(get(fake, "/g/5/settings", Some(&session(9))).await).await;
        assert!(html.contains(MODLOG_STATUS_READY), "{html}");
    }

    #[tokio::test]
    async fn posting_modlog_settings_requires_json() {
        let fake = settings_fake(HistoryAccess::Allowed);
        for ctype in [
            None,
            Some("text/plain"),
            Some("application/x-www-form-urlencoded"),
        ] {
            let r = app(fake.clone())
                .oneshot(post_modlog_req(
                    Some(&session(9)),
                    ctype,
                    Some(ORIGIN),
                    LOG_FORM,
                ))
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE, "{ctype:?}");
        }
        let r = app(fake.clone())
            .oneshot(post_modlog_req(
                Some(&session(9)),
                Some("application/json"),
                Some("https://evil.test"),
                LOG_FORM,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        let r = app(fake.clone())
            .oneshot(post_modlog_req(
                None,
                Some("application/json"),
                Some(ORIGIN),
                LOG_FORM,
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        assert!(fake.modlog_saves.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_managers_may_post_modlog_settings() {
        // A malformed body gets the access answer, not a 400: access is
        // checked before the body is parsed.
        for body_text in [LOG_FORM, "nope"] {
            for (access, status) in [
                (HistoryAccess::Hidden, StatusCode::NOT_FOUND),
                (HistoryAccess::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
                (HistoryAccess::Forbidden, StatusCode::FORBIDDEN),
            ] {
                let fake = settings_fake(access);
                let r = post_modlog(fake.clone(), body_text).await;
                assert_eq!(r.status(), status, "{access:?} {body_text}");
                assert!(fake.modlog_saves.lock().unwrap().is_empty(), "{access:?}");
            }
        }
    }

    #[tokio::test]
    async fn a_manager_saves_modlog_and_the_backend_gets_exactly_that_form() {
        let fake = settings_fake(HistoryAccess::Allowed);
        fake.modlog_view.lock().unwrap().visibility = Some("seen".into());
        let r = post_modlog(fake.clone(), LOG_FORM).await;
        assert_eq!(r.status(), StatusCode::OK);
        let got = body(r).await;
        assert!(got.contains("\"visibility\":\"seen\""), "{got}");
        assert_eq!(
            fake.modlog_saves.lock().unwrap().clone(),
            vec![(
                GuildId::new(5),
                serenity::all::UserId::new(9),
                ModLogSettingsForm {
                    enabled: true,
                    channel: Some("123".into()),
                }
            )]
        );
        // A channel-less, disabled form is valid too.
        let r = post_modlog(fake.clone(), r#"{"enabled":false,"channel":null}"#).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(fake.modlog_saves.lock().unwrap()[1].2.channel, None);
        assert!(!fake.modlog_saves.lock().unwrap()[1].2.enabled);
    }

    /// M26: settings saves draw on the per-user budget the controls use,
    /// rooms and mod log alike, and a refused save reaches no backend.
    #[tokio::test]
    async fn the_sixth_settings_save_in_ten_seconds_is_too_many_and_saves_nothing() {
        let fake = settings_fake(HistoryAccess::Allowed);
        let router = app(fake.clone()); // one router, so one limiter
        let json = Some("application/json");
        let rooms = |user| post_settings_req(Some(&session(user)), json, Some(ORIGIN), FORM);
        let modlog = |user| post_modlog_req(Some(&session(user)), json, Some(ORIGIN), LOG_FORM);
        for (i, req) in [rooms(9), modlog(9), rooms(9), modlog(9), rooms(9)]
            .into_iter()
            .enumerate()
        {
            let r = router.clone().oneshot(req).await.unwrap();
            assert_eq!(r.status(), StatusCode::OK, "save {i}");
        }
        for req in [rooms(9), modlog(9)] {
            let r = router.clone().oneshot(req).await.unwrap();
            assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
            let got = body(r).await;
            assert!(
                got.contains(&format!(
                    "\"error\":{}",
                    serde_json::to_string(DASH_SETTINGS_TOO_MANY).unwrap()
                )),
                "{got}"
            );
        }
        assert_eq!(fake.saves.lock().unwrap().len(), 3);
        assert_eq!(fake.modlog_saves.lock().unwrap().len(), 2);
        // Another user is not held up by it.
        let r = router.oneshot(modlog(10)).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn invalid_modlog_settings_are_400_with_the_reason() {
        for (refused, status, text) in [
            (
                ModLogSaveRefused::Invalid(ModLogSettingsError::NoDatabase),
                StatusCode::BAD_REQUEST,
                "The mod log needs a database.",
            ),
            (
                ModLogSaveRefused::Invalid(ModLogSettingsError::NoChannel),
                StatusCode::BAD_REQUEST,
                "Pick a channel for the log.",
            ),
            (
                ModLogSaveRefused::UnknownChannel,
                StatusCode::BAD_REQUEST,
                DASH_MODLOG_UNKNOWN_CHANNEL,
            ),
            (
                ModLogSaveRefused::Failed,
                StatusCode::SERVICE_UNAVAILABLE,
                MODLOG_SETTINGS_SAVE_FAILED,
            ),
            (
                ModLogSaveRefused::NotLoaded,
                StatusCode::SERVICE_UNAVAILABLE,
                "Mod log settings couldn't be read from the database yet — try again in a minute.",
            ),
        ] {
            let fake = settings_fake(HistoryAccess::Allowed);
            *fake.modlog_save_result.lock().unwrap() = Err(refused);
            let r = post_modlog(fake, LOG_FORM).await;
            assert_eq!(r.status(), status, "{refused:?}");
            let got = body(r).await;
            assert!(
                got.contains(&format!(
                    "\"error\":{}",
                    serde_json::to_string(text).unwrap()
                )),
                "{refused:?}: {got}"
            );
        }
        // Unreadable bodies never reach the backend.
        let fake = settings_fake(HistoryAccess::Allowed);
        for bad in ["{}", "nope", r#"{"enabled":"yes","channel":null}"#] {
            let r = post_modlog(fake.clone(), bad).await;
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert!(fake.modlog_saves.lock().unwrap().is_empty());
    }

    // ---- economy settings ----

    use crate::settings::{
        EconomySaveRefused, EconomySettingsForm, EconomySettingsView, PriceForm, PriceView,
    };
    use crack_core::economy::settings::EconomySettingsError;
    use crack_core::messaging::messages::{
        DASH_ECONOMY_BAD_PRICE, DASH_ECONOMY_GAMBLING_NOTE, DASH_ECONOMY_JAIL_NOTE,
        DASH_ECONOMY_NO_DATABASE, DASH_ECONOMY_SAVE_FAILED, DASH_ECONOMY_SETTINGS_NOT_LOADED,
        DASH_ECONOMY_UNKNOWN_ACTION, DASH_ECONOMY_UNKNOWN_CHANNEL, ECONOMY_DOCS_URL,
    };

    const ECONOMY_FORM: &str = r#"{"enabled":true,"currency_name":"gems","currency_emoji":"X","voice_credits_per_minute":3,"message_credits":4,"message_cooldown_seconds":90,"prices":[{"kind":"immunity","price":77,"on":false}],"immunity_optout":false,"immunity_shop":true,"immunity_mods":false,"optout_cooldown_days":2,"bought_immunity_hours":5,"gambling_enabled":true,"max_bet":9,"rename_hours":48,"short_effect_minutes":7,"jail_channel":"111","announce_channel":null}"#;

    fn post_economy_req(
        cookie: Option<&str>,
        ctype: Option<&str>,
        origin: Option<&str>,
        text: &str,
    ) -> Request<Body> {
        let mut req = Request::post("/g/5/settings/economy");
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

    async fn post_economy(
        fake: std::sync::Arc<FakeBackend>,
        text: &str,
    ) -> axum::response::Response {
        app(fake)
            .oneshot(post_economy_req(
                Some(&session(9)),
                Some("application/json"),
                Some(ORIGIN),
                text,
            ))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn economy_save_403s_for_non_managers() {
        // Access is checked before the body is parsed.
        for body_text in [ECONOMY_FORM, "nope"] {
            for (access, status) in [
                (HistoryAccess::Hidden, StatusCode::NOT_FOUND),
                (HistoryAccess::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
                (HistoryAccess::Forbidden, StatusCode::FORBIDDEN),
            ] {
                let fake = settings_fake(access);
                let r = post_economy(fake.clone(), body_text).await;
                assert_eq!(r.status(), status, "{access:?} {body_text}");
                assert!(fake.economy_saves.lock().unwrap().is_empty(), "{access:?}");
            }
        }
    }

    #[tokio::test]
    async fn economy_save_guards_json_origin_login_and_rate() {
        let fake = settings_fake(HistoryAccess::Allowed);
        let router = app(fake.clone());
        let json = Some("application/json");
        let bad = [
            (
                Some(session(9)),
                Some("text/plain"),
                Some(ORIGIN),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
            (
                Some(session(9)),
                json,
                Some("https://evil.example"),
                StatusCode::FORBIDDEN,
            ),
            (Some(session(9)), json, None, StatusCode::FORBIDDEN),
            (None, json, Some(ORIGIN), StatusCode::UNAUTHORIZED),
        ];
        for (cookie, ctype, origin, status) in bad {
            let r = router
                .clone()
                .oneshot(post_economy_req(
                    cookie.as_deref(),
                    ctype,
                    origin,
                    ECONOMY_FORM,
                ))
                .await
                .unwrap();
            assert_eq!(r.status(), status);
        }
        assert!(fake.economy_saves.lock().unwrap().is_empty());
        let ok = || post_economy_req(Some(&session(9)), json, Some(ORIGIN), ECONOMY_FORM);
        for i in 0..5 {
            let r = router.clone().oneshot(ok()).await.unwrap();
            assert_eq!(r.status(), StatusCode::OK, "save {i}");
        }
        let r = router.clone().oneshot(ok()).await.unwrap();
        assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(fake.economy_saves.lock().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn economy_save_passes_the_exact_form_to_the_backend() {
        let fake = settings_fake(HistoryAccess::Allowed);
        fake.economy_view.lock().unwrap().currency_name = "echoed".into();
        let r = post_economy(fake.clone(), ECONOMY_FORM).await;
        assert_eq!(r.status(), StatusCode::OK);
        let got = body(r).await;
        assert!(got.contains("\"currency_name\":\"echoed\""), "{got}");
        assert_eq!(
            fake.economy_saves.lock().unwrap().clone(),
            vec![(
                GuildId::new(5),
                serenity::all::UserId::new(9),
                EconomySettingsForm {
                    enabled: true,
                    currency_name: "gems".into(),
                    currency_emoji: "X".into(),
                    voice_credits_per_minute: 3,
                    message_credits: 4,
                    message_cooldown_seconds: 90,
                    prices: vec![PriceForm {
                        kind: "immunity".into(),
                        price: 77,
                        on: false,
                    }],
                    immunity_optout: false,
                    immunity_shop: true,
                    immunity_mods: false,
                    optout_cooldown_days: 2,
                    bought_immunity_hours: 5,
                    gambling_enabled: true,
                    max_bet: 9,
                    rename_hours: 48,
                    short_effect_minutes: 7,
                    jail_channel: Some("111".into()),
                    announce_channel: None,
                    coin_withdrawals: None,
                }
            )]
        );
    }

    #[tokio::test]
    async fn economy_save_refuses_an_unknown_channel() {
        let fake = settings_fake(HistoryAccess::Allowed);
        *fake.economy_save_result.lock().unwrap() = Err(EconomySaveRefused::UnknownChannel);
        let r = post_economy(fake, ECONOMY_FORM).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let got = body(r).await;
        assert!(
            got.contains(&serde_json::to_string(DASH_ECONOMY_UNKNOWN_CHANNEL).unwrap()),
            "{got}"
        );
    }

    #[tokio::test]
    async fn economy_save_refuses_an_unknown_action() {
        let fake = settings_fake(HistoryAccess::Allowed);
        *fake.economy_save_result.lock().unwrap() = Err(EconomySaveRefused::UnknownAction);
        let r = post_economy(fake, ECONOMY_FORM).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
        let got = body(r).await;
        assert!(
            got.contains(&serde_json::to_string(DASH_ECONOMY_UNKNOWN_ACTION).unwrap()),
            "{got}"
        );
    }

    #[tokio::test]
    async fn economy_save_shows_validation_messages() {
        for (refused, status, text) in [
            (
                EconomySaveRefused::Invalid(EconomySettingsError::NoDatabase),
                StatusCode::BAD_REQUEST,
                DASH_ECONOMY_NO_DATABASE,
            ),
            (
                EconomySaveRefused::Invalid(EconomySettingsError::BadPrice),
                StatusCode::BAD_REQUEST,
                DASH_ECONOMY_BAD_PRICE,
            ),
            (
                EconomySaveRefused::Failed,
                StatusCode::SERVICE_UNAVAILABLE,
                DASH_ECONOMY_SAVE_FAILED,
            ),
            (
                EconomySaveRefused::NotLoaded,
                StatusCode::SERVICE_UNAVAILABLE,
                DASH_ECONOMY_SETTINGS_NOT_LOADED,
            ),
        ] {
            let fake = settings_fake(HistoryAccess::Allowed);
            *fake.economy_save_result.lock().unwrap() = Err(refused);
            let r = post_economy(fake, ECONOMY_FORM).await;
            assert_eq!(r.status(), status, "{refused:?}");
            let got = body(r).await;
            assert!(
                got.contains(&format!(
                    "\"error\":{}",
                    serde_json::to_string(text).unwrap()
                )),
                "{refused:?}: {got}"
            );
        }
        // Unreadable bodies never reach the backend.
        let fake = settings_fake(HistoryAccess::Allowed);
        for bad in ["{}", "nope", r#"{"enabled":true,"max_bet":null}"#] {
            let r = post_economy(fake.clone(), bad).await;
            assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{bad}");
        }
        assert!(fake.economy_saves.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn settings_page_shows_the_economy_section_with_current_values() {
        let fake = settings_fake(HistoryAccess::Allowed);
        {
            let mut v = fake.economy_view.lock().unwrap();
            *v = EconomySettingsView {
                enabled: true,
                currency_name: "<b>gems</b>".into(),
                currency_emoji: "X".into(),
                voice_credits_per_minute: 3,
                message_credits: 4,
                message_cooldown_seconds: 90,
                prices: vec![PriceView {
                    kind: "immunity".into(),
                    label: "Immunity".into(),
                    price: 77,
                    on: false,
                    hint: None,
                }],
                immunity_optout: true,
                immunity_shop: false,
                immunity_mods: true,
                optout_cooldown_days: 2,
                bought_immunity_hours: 5,
                gambling_enabled: true,
                max_bet: 9,
                rename_hours: 48,
                short_effect_minutes: 7,
                jail_channel: None,
                announce_channel: None,
                voice_channels: vec![],
                text_channels: vec![],
                preflight: vec![],
                announce_findings: None,
                coin_build: false,
                coin_withdrawals: false,
            };
        }
        let r = get(fake, "/g/5/settings", Some(&session(9))).await;
        assert_eq!(r.status(), StatusCode::OK);
        let html = body(r).await;
        for needle in [
            "id=\"economy-form\"",
            "id=\"e-enabled\" checked",
            "id=\"e-name\" maxlength=\"24\" value=\"&lt;b&gt;gems&lt;/b&gt;\"",
            "id=\"e-voice\" min=\"0\" max=\"100\" value=\"3\"",
            "<tr data-kind=\"immunity\"><td>Immunity</td>",
            "class=\"e-price\" aria-label=\"Price\" min=\"1\" max=\"1000000\" value=\"77\"",
            "id=\"e-optout\" checked",
            "id=\"e-shop\"> ",
            "id=\"e-gambling\" checked",
            "id=\"e-max-bet\" min=\"1\" max=\"1000000\" value=\"9\"",
            "id=\"e-rename-hours\" min=\"1\" max=\"168\" value=\"48\"",
            "id=\"e-short\" min=\"1\" max=\"60\" value=\"7\"",
            "Credits have no cash value. Only turn this on where your members are adults.",
            ECONOMY_DOCS_URL,
        ] {
            assert!(html.contains(needle), "missing {needle}: {html}");
        }
        // The price row's "for sale" box is unchecked.
        assert!(
            html.contains("class=\"e-price-on\" aria-label=\"For sale\"><"),
            "{html}"
        );
        assert_eq!(
            DASH_ECONOMY_GAMBLING_NOTE,
            "Credits have no cash value. Only turn this on where your members are adults."
        );
        // The note sits under the gambling switch, the docs link comes last.
        let at = |n: &str| html.find(n).unwrap();
        assert!(at("id=\"e-gambling\"") < at("id=\"e-gambling-note\""));
        assert!(at("id=\"e-gambling-note\"") < at(ECONOMY_DOCS_URL));
        assert!(at("id=\"modlog-form\"") < at("id=\"economy-form\""));
    }

    #[tokio::test]
    async fn settings_page_shows_the_jail_note_and_preflight() {
        use crate::settings::{ActionPreflight, ChannelOption};
        let fake = settings_fake(HistoryAccess::Allowed);
        {
            let mut v = fake.economy_view.lock().unwrap();
            v.jail_channel = Some("111".into());
            v.announce_channel = Some("222".into());
            v.voice_channels = vec![
                ChannelOption {
                    id: "110".into(),
                    name: "Lobby".into(),
                },
                ChannelOption {
                    id: "111".into(),
                    name: "Brig <b>".into(),
                },
            ];
            v.text_channels = vec![ChannelOption {
                id: "222".into(),
                name: "news".into(),
            }];
            v.preflight = vec![
                ActionPreflight {
                    kind: "rename".into(),
                    label: "Rename".into(),
                    needs: Some(vec![]),
                },
                ActionPreflight {
                    kind: "lock".into(),
                    label: "Lock".into(),
                    needs: Some(vec![
                        "Give CrackTunes View Audit Log in Server Settings".into()
                    ]),
                },
                ActionPreflight {
                    kind: "mute".into(),
                    label: "Mute".into(),
                    needs: None,
                },
            ];
            v.announce_findings = Some(vec![]);
        }
        let r = get(fake, "/g/5/settings", Some(&session(9))).await;
        let html = body(r).await;
        assert_eq!(
            DASH_ECONOMY_JAIL_NOTE,
            "Members are moved here even if they can't see it."
        );
        for needle in [
            "<option value=\"111\" selected>Brig &lt;b&gt;</option>",
            "<option value=\"110\">Lobby</option>",
            "<option value=\"222\" selected>news</option>",
            "<p id=\"e-jail-note\">Members are moved here even if they can't see it.</p>",
            "<li data-kind=\"rename\"><strong>Rename</strong>: <span>Ready.</span></li>",
            "<li data-kind=\"lock\"><strong>Lock</strong>: <span>Give CrackTunes View Audit Log in Server Settings</span></li>",
            "<li data-kind=\"mute\"><strong>Mute</strong>: <span>Couldn't check permissions right now.</span></li>",
            "<li data-kind=\"announce\"><strong>Announcements</strong>: <span>Ready.</span></li>",
        ] {
            let escaped = needle.replace('\'', "&#39;");
            assert!(html.contains(&escaped), "missing {needle}: {html}");
        }
        let at = |n: &str| html.find(n).unwrap();
        assert!(at("id=\"e-jail\"") < at("id=\"e-jail-note\""));
        assert!(at("id=\"e-jail-note\"") < at("id=\"e-announce\""));
    }

    #[tokio::test]
    async fn queue_page_links_settings_only_for_managers() {
        for (access, linked) in [
            (HistoryAccess::Allowed, true),
            (HistoryAccess::Forbidden, false),
        ] {
            let fake = history_fake(access);
            let html = body(get(fake, "/g/5", Some(&session(9))).await).await;
            assert_eq!(html.contains("href=\"/g/5/settings\""), linked, "{html}");
            assert_eq!(html.contains("href=\"/g/5/history\""), linked, "{html}");
        }
    }

    #[tokio::test]
    async fn settings_js_is_served_as_javascript() {
        let r = get(member_viewing(), "/assets/settings.js", None).await;
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(
            r.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
    }
}
