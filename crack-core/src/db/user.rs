use ::chrono::Duration;
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

#[derive(sqlx::FromRow)]
pub struct UserVote {
    pub id: i64,
    pub user_id: i64,
    pub site: String,
    pub timestamp: chrono::NaiveDateTime,
}

impl User {
    pub async fn insert_test_user(pool: &PgPool, user_id: Option<i64>, username: Option<String>) {
        let user = username.unwrap_or("test".to_string());
        let user_id = user_id.unwrap_or(1);
        let result = sqlx::query!(
            r#"insert into public.user
            (id, username, avatar_url, bot, created_at, updated_at, last_seen) values ($1, $2, '', false, now(), now(), now())"#,
            user_id,
            user,
        )
        .execute(pool)
        .await;

        match result {
            Ok(_) => println!("User inserted successfully."),
            Err(e) => println!("Failed to insert user: {}", e),
        }
    }

    pub async fn get_user(pool: &PgPool, user_id: i64) -> Option<User> {
        let result = sqlx::query_as!(User, r#"SELECT * FROM public.user WHERE id = $1"#, user_id)
            .fetch_optional(pool)
            .await;

        match result {
            Ok(user) => Some(user?),
            Err(e) => {
                println!("Failed to get user: {}", e);
                None
            },
        }
    }

    pub async fn insert_or_update_user(
        pool: &PgPool,
        user_id: i64,
        username: String,
    ) -> Result<User, sqlx::Error> {
        sqlx::query_as!(
            User,
            r#"INSERT INTO public.user (id, username, discriminator, avatar_url, bot, created_at, updated_at, last_seen)
            VALUES ($1, $2, 0, '', false, now(), now(), now())
            ON CONFLICT (id) DO UPDATE SET last_seen = now(), username = $2
            RETURNING id, username, discriminator, avatar_url, bot, created_at, updated_at, last_seen
            "#,
            user_id,
            username,
        )
        .fetch_one(pool)
        .await
    }
}

impl UserVote {
    pub async fn insert_user_vote(
        pool: &PgPool,
        user_id: i64,
        site: String,
    ) -> Result<UserVote, sqlx::Error> {
        sqlx::query_as!(
            UserVote,
            r#"INSERT INTO user_votes (user_id, site, timestamp)
            VALUES ($1, $2, now())
            RETURNING id, user_id, site, timestamp
            "#,
            user_id,
            site,
        )
        .fetch_one(pool)
        .await
    }

    pub async fn has_voted_recently(
        user_id: i64,
        site_name: String,
        duration: chrono::NaiveDateTime,
        pool: &PgPool,
    ) -> bool {
        let result = sqlx::query_as!(
            UserVote,
            "SELECT * FROM user_votes WHERE user_id = $1 AND timestamp > $2 AND site = $3",
            user_id,
            duration,
            site_name
        )
        .fetch_optional(pool)
        .await
        .expect("Failed to execute query");

        result.is_some()
    }

    pub async fn has_voted_recently_topgg(user_id: i64, pool: &PgPool) -> bool {
        let twelve_hours_ago = chrono::Utc::now().naive_utc() - Duration::hours(12);
        let site_name = "top.gg".to_string(); // Define the site you are checking for votes from

        Self::has_voted_recently(user_id, site_name, twelve_hours_ago, pool).await
    }
}
