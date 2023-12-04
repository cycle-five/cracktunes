use sqlx::types::chrono::{self};

pub struct PlayLog {
    pub id: i64,
    pub user_id: i64,
    pub guild_id: i64,
    pub metadata_id: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
