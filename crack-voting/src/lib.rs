use dbl::types::Webhook;
use lazy_static::lazy_static;
use sqlx::PgPool;
use std::env;
use std::sync::Arc;
use warp::{body::BodyDeserializeError, http::StatusCode, path, reject, Filter, Rejection, Reply};

const WEBHOOK_SECRET_DEFAULT: &str = "test_secret";
const DATABASE_URL_DEFAULT: &str = "postgresql://postgres:postgres@localhost:5432/postgres";

lazy_static! {
    static ref WEBHOOK_SECRET: String =
        env::var("WEBHOOK_SECRET").unwrap_or(WEBHOOK_SECRET_DEFAULT.to_string());
    static ref DATABASE_URL: String =
        env::var("DATABASE_URL").unwrap_or(DATABASE_URL_DEFAULT.to_string());
}

/// Struct to hold the context for the voting server.
#[derive(Debug, Clone)]
pub struct VotingContext {
    pool: Arc<PgPool>,
    secret: &'static str,
}

/// Implement the `VotingContext`.
impl VotingContext {
    async fn new() -> Self {
        let pool = sqlx::PgPool::connect(&DATABASE_URL)
            .await
            .expect("failed to connect to database");
        let secret = get_secret();
        VotingContext {
            pool: Arc::new(pool),
            secret,
        }
    }

    pub async fn new_with_pool(pool: sqlx::PgPool) -> Self {
        let secret = get_secret();
        VotingContext {
            pool: Arc::new(pool),
            secret,
        }
    }
}

/// NewClass for the Webhook to store in the database.
#[derive(Debug, serde::Deserialize, serde::Serialize, sqlx::FromRow, Clone, PartialEq, Eq)]
pub struct CrackedWebhook {
    webhook: Webhook,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// Custom error type for unauthorized requests.
#[derive(Debug)]
struct Unauthorized;

impl warp::reject::Reject for Unauthorized {}

impl std::fmt::Display for Unauthorized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Unauthorized")
    }
}

impl std::error::Error for Unauthorized {}

/// Custom error type for unauthorized requests.
#[derive(Debug)]
struct Sqlx(sqlx::Error);

impl warp::reject::Reject for Sqlx {}

impl std::fmt::Display for Sqlx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

impl std::error::Error for Sqlx {}
/// Get the webhook secret from the environment.
fn get_secret() -> &'static str {
    &WEBHOOK_SECRET
}

fn webhook_type_to_string(kind: dbl::types::WebhookType) -> String {
    match kind {
        dbl::types::WebhookType::Upvote => "upvote".to_string(),
        dbl::types::WebhookType::Test => "test".to_string(),
    }
}

/// Write the received webhook to the database.
async fn write_webhook_to_db(ctx: VotingContext, webhook: Webhook) -> Result<(), sqlx::Error> {
    // Check for the user in the database, since we have a foreign key constraint.
    // Create the user if they don't exist.
    let res = sqlx::query!(
        r#"INSERT INTO "user"
            (id, username, discriminator, avatar_url, bot, created_at, updated_at, last_seen)
        VALUES
            ($1, 'NULL', 0, 'NULL', false, now(), now(), now())
        ON CONFLICT (id)
        DO UPDATE SET last_seen = now()
        "#,
        webhook.user.0 as i64,
    )
    .execute(ctx.pool.as_ref())
    .await;
    if let Err(e) = res {
        eprintln!("Failed to insert / update user: {}", e);
        return Err(e);
    }
    //let executor = ctx.pool.clone();
    let res = sqlx::query!(
        r#"INSERT INTO vote_webhook
            (bot_id, user_id, kind, is_weekend, query, created_at)
        VALUES
            ($1, $2, $3::WEBHOOK_KIND, $4, $5, now())
        "#,
        webhook.bot.0 as i64,
        webhook.user.0 as i64,
        webhook_type_to_string(webhook.kind) as _,
        webhook.is_weekend,
        webhook.query,
    )
    .execute(ctx.pool.as_ref())
    .await;
    match res {
        Ok(_) => println!("Webhook written to database"),
        Err(e) => {
            eprintln!("Failed to write webhook to database: {}", e);
            return Err(e);
        },
    }
    Ok(())
}

/// Create a filter that checks the `Authorization` header against the secret.
fn header(secret: &str) -> impl Filter<Extract = (), Error = Rejection> + Clone + '_ {
    warp::header::<String>("authorization")
        .and_then(move |val: String| async move {
            if val == secret {
                println!("Authorized");
                Ok(())
            } else {
                println!("Not Authorized");
                Err(reject::custom(Unauthorized))
            }
        })
        .untuple_one()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReplyBody {
    body: String,
}

/// Async function to process the received webhook.
async fn process_webhook(ctx: VotingContext, hook: Webhook) -> Result<impl Reply, Rejection> {
    write_webhook_to_db(ctx, hook.clone()).await.map_err(Sqlx)?;
    Ok(warp::reply::json(&ReplyBody {
        body: "Success.".to_string(),
    }))
}

/// Create a filter that handles the webhook.
async fn get_webhook(
    ctx: VotingContext,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let secret = ctx.secret;
    let context = warp::any().map(move || ctx.clone());

    warp::post()
        .and(path!("dbl" / "webhook"))
        .and(header(secret))
        .and(warp::body::json())
        .and(context)
        .and_then(
            |hook: Webhook, ctx: VotingContext| async move { process_webhook(ctx, hook).await },
        )
        .recover(custom_error)
}

/// Get the routes for the server.
async fn get_app(
    ctx: VotingContext,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    println!("get_app");
    let webhook = get_webhook(ctx).await;
    let health = warp::path!("health").map(|| "Hello, world!");
    webhook.or(health)
}

/// Run the server.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = VotingContext::new().await; //Box::leak(Box::new(VotingContext::new().await));
    let app = get_app(ctx).await;

    warp::serve(app).run(([0, 0, 0, 0], 3030)).await;

    Ok(())
}

/// Custom error handling for the server.
async fn custom_error(err: Rejection) -> Result<impl Reply, Rejection> {
    eprintln!("Error: {:?}", err);
    if err.find::<BodyDeserializeError>().is_some() {
        Ok(warp::reply::with_status(
            warp::reply(),
            StatusCode::BAD_REQUEST,
        ))
    } else if err.find::<Unauthorized>().is_some() {
        Ok(warp::reply::with_status(
            warp::reply(),
            StatusCode::UNAUTHORIZED,
        ))
    } else {
        Err(err)
    }
}

#[cfg(test)]
mod test {
    use serde_json;
    use sqlx::{Pool, Postgres};

    use crate::get_secret;
    use crate::{StatusCode, VotingContext, Webhook};

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    use super::*;

    #[sqlx::test(migrator = "MIGRATOR")]
    async fn test_voting_context_creation(pool: PgPool) {
        let secret = "test_secret";
        std::env::set_var("WEBHOOK_SECRET", secret);

        let context = VotingContext::new_with_pool(pool).await;

        assert_eq!(context.secret, secret);
        // We can't directly compare PgPools, but we can check if it's initialized
        assert!(context.pool.acquire().await.is_ok());
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    //#[sqlx::test]
    async fn test_bad_req(_pool: Pool<Postgres>) {
        let ctx = VotingContext::new().await;
        let secret = get_secret();
        println!("Secret {}", secret);
        let app = get_app(ctx).await;

        let res = warp::test::request()
            .method("POST")
            .path("/dbl/webhook")
            .header("authorization", secret)
            .body("bad json")
            .reply(&app)
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    //#[sqlx::test]
    async fn test_authorized(pool: Pool<Postgres>) {
        let ctx = Box::leak(Box::new(VotingContext::new_with_pool(pool).await));
        let secret = get_secret();
        let webhook = &Webhook {
            bot: dbl::types::BotId(11),
            user: dbl::types::UserId(31),
            kind: dbl::types::WebhookType::Test,
            is_weekend: false,
            query: Some("test".to_string()),
        };
        let json_str = serde_json::to_string(webhook).unwrap();
        println!("Secret {}", secret);
        println!("Webhook {}", json_str);
        let res = warp::test::request()
            .method("POST")
            .path("/dbl/webhook")
            .header("authorization", secret)
            .json(&Webhook {
                bot: dbl::types::BotId(11),
                user: dbl::types::UserId(1),
                kind: dbl::types::WebhookType::Test,
                is_weekend: false,
                query: Some("test".to_string()),
            })
            .reply(&get_app(ctx.clone()).await)
            .await;
        assert_eq!(res.status(), StatusCode::OK);
    }
}
