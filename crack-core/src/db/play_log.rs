use sqlx::types::chrono::NaiveDateTime;
use sqlx::{Error, PgPool};

#[derive(Debug, Clone)]
pub struct PlayLog {
    pub id: i64,
    pub user_id: i64,
    pub guild_id: i64,
    pub metadata_id: i64,
    pub created_at: NaiveDateTime,
}

impl PlayLog {
    pub async fn create(
        conn: &PgPool,
        user_id: i64,
        guild_id: i64,
        metadata_id: i64,
    ) -> Result<Self, Error> {
        let play_log = sqlx::query_as!(
            PlayLog,
            r#"
            INSERT INTO play_log (user_id, guild_id, metadata_id)
            VALUES ($1, $2, $3)
            RETURNING id, user_id, guild_id, metadata_id, created_at
            "#,
            user_id,
            guild_id,
            metadata_id
        )
        .fetch_one(conn)
        .await?;
        Ok(play_log)
    }
}
