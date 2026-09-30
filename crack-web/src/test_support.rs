//! A fake backend that records what the routes asked of it, and an app
//! wired to it.

use crate::{
    access::{Membership, Presence},
    backend::{Backend, GuildEntry, MoveRefused},
    routes::{router, WebState},
    view::QueueView,
    watch::ViewSource,
};
use axum::Router;
use serenity::all::{ChannelId, GuildId, UserId};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    Arc, Mutex,
};
use uuid::Uuid;

pub const ORIGIN: &str = "https://dash.test";
pub const JWT_SECRET: &str = "test-secret-test-secret-test-secret!";
pub const BOT_CHANNEL: ChannelId = ChannelId::new(77);

pub struct FakeBackend {
    pub membership: Mutex<Membership>,
    pub user_channel: Mutex<Option<ChannelId>>,
    pub view: Mutex<QueueView>,
    pub move_result: Mutex<Result<usize, MoveRefused>>,
    /// What `view` becomes once a move succeeds; `None` leaves it as it was.
    pub view_after_move: Mutex<Option<QueueView>>,
    /// Every move the routes asked for: (guild, track, to).
    pub moves: Mutex<Vec<(GuildId, Uuid, usize)>>,
    pub guilds: Vec<GuildEntry>,
    /// How many times the routes and the hub read presence and the view.
    pub presence_calls: AtomicUsize,
    pub view_calls: AtomicUsize,
    /// Make `presence` never answer, as a stalled Discord call would.
    pub presence_hangs: AtomicBool,
}

impl FakeBackend {
    pub fn new(
        membership: Membership,
        user_channel: Option<ChannelId>,
        view: QueueView,
    ) -> Arc<Self> {
        Arc::new(Self {
            membership: Mutex::new(membership),
            user_channel: Mutex::new(user_channel),
            view: Mutex::new(view),
            move_result: Mutex::new(Ok(0)),
            view_after_move: Mutex::new(None),
            moves: Mutex::new(Vec::new()),
            presence_calls: AtomicUsize::new(0),
            view_calls: AtomicUsize::new(0),
            presence_hangs: AtomicBool::new(false),
            guilds: vec![GuildEntry {
                id: GuildId::new(5),
                name: "Five".into(),
                channel: Some("Music".into()),
            }],
        })
    }

    pub fn move_count(&self) -> usize {
        self.moves.lock().unwrap().len()
    }
}

impl ViewSource for FakeBackend {
    async fn view(&self, _g: GuildId) -> QueueView {
        self.view_calls.fetch_add(1, SeqCst);
        self.view.lock().unwrap().clone()
    }
}

impl Backend for FakeBackend {
    async fn presence(&self, _g: GuildId, _u: UserId) -> Presence {
        self.presence_calls.fetch_add(1, SeqCst);
        if self.presence_hangs.load(SeqCst) {
            std::future::pending::<()>().await;
        }
        Presence {
            membership: *self.membership.lock().unwrap(),
            user_channel: *self.user_channel.lock().unwrap(),
            bot_channel: Some(BOT_CHANNEL),
        }
    }

    async fn move_track(&self, g: GuildId, id: Uuid, to: usize) -> Result<usize, MoveRefused> {
        self.moves.lock().unwrap().push((g, id, to));
        // A real move takes a moment (the queue lease, the call lock): long
        // enough for anything the routes set going early to run first.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        let result = *self.move_result.lock().unwrap();
        if result.is_ok() {
            if let Some(v) = self.view_after_move.lock().unwrap().take() {
                *self.view.lock().unwrap() = v;
            }
        }
        result
    }

    async fn guilds_for(&self, _u: UserId) -> Vec<GuildEntry> {
        self.guilds.clone()
    }

    fn guild_name(&self, _g: GuildId) -> Option<String> {
        Some("Five".into())
    }
}

pub fn state(fake: Arc<FakeBackend>) -> WebState<FakeBackend> {
    let mut config = crate::config::WebEnv {
        client_id: "1".into(),
        client_secret: "s".into(),
        public_origin: ORIGIN.into(),
        jwt_secret: JWT_SECRET.into(),
        bot_token: "t".into(),
        bind: "127.0.0.1:0".into(),
    }
    .catacombs_config();
    config.security.jwt_secret = JWT_SECRET.into();
    WebState {
        auth: Arc::new(catacombs::AppState::new(
            config,
            catacombs::MemoryStorage::new(),
        )),
        hub: crate::watch::Hub::new(fake.clone(), crate::watch::TICK, crate::watch::LINGER),
        backend: fake,
        origin: ORIGIN.into(),
    }
}

pub fn app(fake: Arc<FakeBackend>) -> Router {
    router(state(fake))
}

/// A `Cookie` header value logging in as `user`.
pub fn session(user: u64) -> String {
    let jwt = catacombs::auth::generate_token(user as i64, "tester", JWT_SECRET).unwrap();
    format!("catacombs_session={jwt}")
}

pub async fn body(resp: axum::response::Response) -> String {
    use http_body_util::BodyExt;
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}
