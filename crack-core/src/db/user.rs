use sqlx::{
    types::chrono::{self},
    PgPool,
};

#[derive(Debug, Default)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub discriminator: Option<i16>,
    pub avatar_url: String,
    pub bot: bool,
    pub created_at: chrono::NaiveDate,
    pub updated_at: chrono::NaiveDate,
    pub last_seen: chrono::NaiveDate,
}

impl User {
    pub async fn get_user(pool: &PgPool, user_id: i64) -> Option<User> {
        sqlx::query_as!(User, r#"SELECT * FROM "user" WHERE id = $1"#, user_id)
            .fetch_optional(pool)
            .await
            .ok()?
    }

    pub async fn insert_user(
        pool: &PgPool,
        user_id: i64,
        username: String,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"INSERT INTO "user" (id, username) VALUES ($1, $2)"#,
            user_id,
            username,
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
