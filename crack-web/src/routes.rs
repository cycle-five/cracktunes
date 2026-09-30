//! The dashboard's HTTP surface.

use crate::{
    access::{decide, Access},
    backend::Backend,
    page,
    view::PageState,
    watch::Hub,
};
use axum::{
    extract::{FromRef, Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use catacombs::auth::AuthenticatedUser;
use serenity::all::{GuildId, UserId};
use std::{num::NonZeroU64, sync::Arc, time::Duration};
use tower_http::timeout::TimeoutLayer;

/// Everything but the stylesheet and two scripts is off; no framing.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// How long any non-streaming request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

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
        .layer(timeout);
    Router::new()
        .merge(timed)
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
}
