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
            Ok(user) => Some(user),
            Err(e) => {
                println!("Failed to get user: {}", e);
                None
            }
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
