//! A fake backend that records what the routes asked of it, and an app
//! wired to it.

use crate::{
    access::{Membership, Presence},
    backend::{
        AddRefused, At, Backend, Control, ControlRefused, GuildEntry, MoveRefused, Query,
        SavedTrack, Source, SourceResult, SourceSet,
    },
    routes::{router, WebState},
    settings::{
        EconomySaveRefused, EconomySettingsForm, EconomySettingsView, ModLogSaveRefused,
        ModLogSettingsForm, ModLogSettingsView, RoomSettingsForm, RoomSettingsView, SaveRefused,
    },
    view::{PlanView, QueueView, TrackView},
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

/// A search the routes made: (guild, query text, sources, since).
pub type SearchCall = (
    GuildId,
    String,
    SourceSet,
    Option<chrono::DateTime<chrono::Utc>>,
);
/// An add the routes asked for: (guild, user, name, track, at).
pub type AddCall = (GuildId, UserId, String, SavedTrack, At);

pub struct FakeBackend {
    pub membership: Mutex<Membership>,
    pub user_channel: Mutex<Option<ChannelId>>,
    pub view: Mutex<QueueView>,
    pub move_result: Mutex<Result<usize, MoveRefused>>,
    /// What `view` becomes once a move succeeds; `None` leaves it as it was.
    pub view_after_move: Mutex<Option<QueueView>>,
    /// Every move the routes asked for: (guild, track, to).
    pub moves: Mutex<Vec<(GuildId, Uuid, usize)>>,
    /// The user the last move was made for.
    pub mover: Mutex<Option<UserId>>,
    pub plan: Mutex<PlanView>,
    pub control_result: Mutex<Result<(), ControlRefused>>,
    /// Every control the routes asked for.
    pub controls: Mutex<Vec<(GuildId, UserId, Control)>>,
    /// What `view` becomes once a control succeeds; `None` leaves it as it was.
    pub view_after_control: Mutex<Option<QueueView>>,
    /// How many times the routes read the plan.
    pub plan_calls: AtomicUsize,
    /// Make `plan` never answer, as a stalled settings read would.
    pub plan_hangs: AtomicBool,
    pub guilds: Vec<GuildEntry>,
    /// How many times the routes and the hub read presence and the view.
    pub presence_calls: AtomicUsize,
    pub view_calls: AtomicUsize,
    /// Make `presence` never answer, as a stalled Discord call would.
    pub presence_hangs: AtomicBool,
    pub history_access: Mutex<crate::access::HistoryAccess>,
    /// Make `history_access` never answer, as a stalled Discord call would.
    pub history_access_hangs: AtomicBool,
    pub history_result: Mutex<Result<crate::history::HistoryPage, crate::backend::HistoryError>>,
    /// Every history query the routes made.
    pub history_queries: Mutex<Vec<crate::history::HistoryQuery>>,
    /// What `search` answers.
    pub search_result: Mutex<Vec<(Source, SourceResult)>>,
    /// Every search the routes made: (guild, query text, sources, since).
    pub searches: Mutex<Vec<SearchCall>>,
    pub add_result: Mutex<Result<(), AddRefused>>,
    /// Every add the routes asked for: (guild, user, name, track, at).
    pub adds: Mutex<Vec<AddCall>>,
    /// What `view` becomes once an add succeeds; `None` leaves it as it was.
    pub view_after_add: Mutex<Option<QueueView>>,
    /// What `room_settings` answers.
    pub room_view: Mutex<RoomSettingsView>,
    pub save_result: Mutex<Result<(), SaveRefused>>,
    /// Every settings save the routes asked for: (guild, user, form).
    pub saves: Mutex<Vec<(GuildId, UserId, RoomSettingsForm)>>,
    /// What `mod_log_settings` answers.
    pub modlog_view: Mutex<ModLogSettingsView>,
    pub modlog_save_result: Mutex<Result<(), ModLogSaveRefused>>,
    /// Every mod log save the routes asked for: (guild, user, form).
    pub modlog_saves: Mutex<Vec<(GuildId, UserId, ModLogSettingsForm)>>,
    /// What `economy_settings` answers.
    pub economy_view: Mutex<EconomySettingsView>,
    pub economy_save_result: Mutex<Result<(), EconomySaveRefused>>,
    /// Every economy save the routes asked for: (guild, user, form).
    pub economy_saves: Mutex<Vec<(GuildId, UserId, EconomySettingsForm)>>,
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
            mover: Mutex::new(None),
            presence_calls: AtomicUsize::new(0),
            view_calls: AtomicUsize::new(0),
            presence_hangs: AtomicBool::new(false),
            history_access: Mutex::new(crate::access::HistoryAccess::Forbidden),
            history_access_hangs: AtomicBool::new(false),
            history_result: Mutex::new(Ok(crate::history::HistoryPage::default())),
            history_queries: Mutex::new(Vec::new()),
            search_result: Mutex::new(Vec::new()),
            searches: Mutex::new(Vec::new()),
            add_result: Mutex::new(Ok(())),
            adds: Mutex::new(Vec::new()),
            view_after_add: Mutex::new(None),
            room_view: Mutex::new(RoomSettingsView {
                enabled: false,
                hub: None,
                name_template: "{name}'s room".into(),
                default_limit: 0,
                empty_grace_secs: 30,
                voice_channels: Vec::new(),
                findings: None,
            }),
            save_result: Mutex::new(Ok(())),
            saves: Mutex::new(Vec::new()),
            modlog_view: Mutex::new(ModLogSettingsView {
                enabled: false,
                channel: None,
                text_channels: Vec::new(),
                findings: None,
                visibility: None,
            }),
            modlog_save_result: Mutex::new(Ok(())),
            modlog_saves: Mutex::new(Vec::new()),
            economy_view: Mutex::new(EconomySettingsView::from_settings(
                &crack_core::economy::settings::EconomySettings::off(GuildId::new(5)),
            )),
            economy_save_result: Mutex::new(Ok(())),
            economy_saves: Mutex::new(Vec::new()),
            plan: Mutex::new(PlanView::Premium),
            control_result: Mutex::new(Ok(())),
            controls: Mutex::new(Vec::new()),
            view_after_control: Mutex::new(None),
            plan_calls: AtomicUsize::new(0),
            plan_hangs: AtomicBool::new(false),
            guilds: vec![GuildEntry {
                id: GuildId::new(5),
                name: "Five".into(),
                channel: Some("Music".into()),
            }],
        })
    }

    pub fn last_mover(&self) -> Option<UserId> {
        *self.mover.lock().unwrap()
    }

    pub fn control_count(&self) -> usize {
        self.controls.lock().unwrap().len()
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

    async fn move_track(
        &self,
        user: UserId,
        g: GuildId,
        id: Uuid,
        to: usize,
    ) -> Result<usize, MoveRefused> {
        *self.mover.lock().unwrap() = Some(user);
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

    async fn control(&self, user: UserId, g: GuildId, c: Control) -> Result<(), ControlRefused> {
        self.controls.lock().unwrap().push((g, user, c));
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        let result = *self.control_result.lock().unwrap();
        if result.is_ok() {
            if let Some(v) = self.view_after_control.lock().unwrap().take() {
                *self.view.lock().unwrap() = v;
            }
        }
        result
    }

    async fn plan(&self, _g: GuildId) -> PlanView {
        self.plan_calls.fetch_add(1, SeqCst);
        if self.plan_hangs.load(SeqCst) {
            std::future::pending::<()>().await;
        }
        *self.plan.lock().unwrap()
    }

    async fn guilds_for(&self, _u: UserId) -> Vec<GuildEntry> {
        self.guilds.clone()
    }

    fn guild_name(&self, _g: GuildId) -> Option<String> {
        Some("Five".into())
    }

    async fn history_access(&self, _g: GuildId, _u: UserId) -> crate::access::HistoryAccess {
        if self.history_access_hangs.load(SeqCst) {
            std::future::pending::<()>().await;
        }
        *self.history_access.lock().unwrap()
    }

    async fn history(
        &self,
        _g: GuildId,
        q: &crate::history::HistoryQuery,
    ) -> Result<crate::history::HistoryPage, crate::backend::HistoryError> {
        self.history_queries.lock().unwrap().push(q.clone());
        self.history_result.lock().unwrap().clone()
    }

    async fn search(
        &self,
        g: GuildId,
        q: &Query,
        sources: SourceSet,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Vec<(Source, SourceResult)> {
        self.searches
            .lock()
            .unwrap()
            .push((g, q.as_str().to_owned(), sources, since));
        self.search_result.lock().unwrap().clone()
    }

    async fn add(
        &self,
        user: UserId,
        name: &str,
        g: GuildId,
        track: SavedTrack,
        at: At,
    ) -> Result<(), AddRefused> {
        self.adds
            .lock()
            .unwrap()
            .push((g, user, name.to_owned(), track, at));
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        let result = *self.add_result.lock().unwrap();
        if result.is_ok() {
            if let Some(v) = self.view_after_add.lock().unwrap().take() {
                *self.view.lock().unwrap() = v;
            }
        }
        result
    }

    async fn room_settings(&self, _g: GuildId) -> RoomSettingsView {
        self.room_view.lock().unwrap().clone()
    }

    async fn save_room_settings(
        &self,
        user: UserId,
        g: GuildId,
        f: RoomSettingsForm,
    ) -> Result<RoomSettingsView, SaveRefused> {
        self.saves.lock().unwrap().push((g, user, f));
        (*self.save_result.lock().unwrap())?;
        Ok(self.room_view.lock().unwrap().clone())
    }

    async fn mod_log_settings(&self, _g: GuildId) -> ModLogSettingsView {
        self.modlog_view.lock().unwrap().clone()
    }

    async fn save_mod_log_settings(
        &self,
        user: UserId,
        g: GuildId,
        f: ModLogSettingsForm,
    ) -> Result<ModLogSettingsView, ModLogSaveRefused> {
        self.modlog_saves.lock().unwrap().push((g, user, f));
        (*self.modlog_save_result.lock().unwrap())?;
        Ok(self.modlog_view.lock().unwrap().clone())
    }

    async fn economy_settings(&self, _g: GuildId) -> EconomySettingsView {
        self.economy_view.lock().unwrap().clone()
    }

    async fn save_economy_settings(
        &self,
        user: UserId,
        g: GuildId,
        f: EconomySettingsForm,
    ) -> Result<EconomySettingsView, EconomySaveRefused> {
        self.economy_saves.lock().unwrap().push((g, user, f));
        (*self.economy_save_result.lock().unwrap())?;
        Ok(self.economy_view.lock().unwrap().clone())
    }
}

pub fn state(fake: Arc<FakeBackend>) -> WebState<FakeBackend> {
    let mut config = crate::config::WebEnv {
        client_id: "1".into(),
        client_secret: "s".into(),
        public_origin: ORIGIN.into(),
        jwt_secret: JWT_SECRET.into(),
        bind: "127.0.0.1:0".into(),
    }
    .catacombs_config();
    config.security.jwt_secret = JWT_SECRET.into();
    WebState {
        auth: Arc::new(catacombs::Auth::new(config, catacombs::MemoryStorage::new()).unwrap()),
        hub: crate::watch::Hub::new(fake.clone(), crate::watch::TICK, crate::watch::LINGER),
        backend: fake,
        origin: ORIGIN.into(),
        limiter: Arc::new(crate::limit::RateLimit::new(
            crate::limit::CONTROLS_PER_WINDOW,
            crate::limit::CONTROL_WINDOW,
        )),
        hits: Arc::new(crate::hits::HitMemo::new(crate::hits::HIT_TTL)),
        search_limiter: Arc::new(crate::limit::RateLimit::new(
            crate::limit::SEARCHES_PER_WINDOW,
            crate::limit::SEARCH_WINDOW,
        )),
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

/// A queue with a playing track and two upcoming.
pub fn playing() -> QueueView {
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
        paused: false,
        looping: false,
    }
}

/// A member in the bot's channel, with [`playing`] on.
pub fn controller() -> Arc<FakeBackend> {
    FakeBackend::new(Membership::Member, Some(BOT_CHANNEL), playing())
}
