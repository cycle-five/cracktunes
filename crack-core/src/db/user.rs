use songbird::tracks::TrackHandle;
use sqlx::{
    types::chrono::{self},
    SqlitePool,
};

use crate::CrackedError;

#[derive(Debug, Default)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub discriminator: Option<i64>,
    pub avatar_url: String,
    pub bot: bool,
    pub created_at: chrono::NaiveDate,
    pub updated_at: chrono::NaiveDate,
    pub last_seen: chrono::NaiveDate,
}

impl User {
    pub async fn get_user(pool: &SqlitePool, user_id: i64) -> Option<User> {
        sqlx::query_as!(User, "SELECT * FROM user WHERE id = ?", user_id)
            .fetch_optional(pool)
            .await
            .ok()?
    }

    pub async fn insert_user(
        pool: &SqlitePool,
        user_id: i64,
        username: String,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO user (id, username) VALUES (?, ?)",
            user_id,
            username
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
