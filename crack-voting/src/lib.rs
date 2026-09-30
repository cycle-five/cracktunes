use dbl::types::Webhook;
use lazy_static::lazy_static;
use sqlx::PgPool;
use std::sync::Arc;
use std::{convert::Infallible, env};
use warp::{
    body::BodyDeserializeError,
    http::{
        header::{HeaderName, HeaderValue, AUTHORIZATION},
        HeaderMap, StatusCode,
    },
    path, reject, Filter, Rejection, Reply,
};

const DATABASE_URL_DEFAULT: &str = "postgresql://postgres:postgres@localhost:5432/postgres";

/// Largest webhook body accepted. A top.gg vote is well under 1 KiB.
const MAX_BODY_BYTES: u64 = 16 * 1024;

/// 🔒 Secrets that are published in this repo, and so are no secret at all.
/// `test_secret` was the built-in fallback *and* the compose default, which
/// meant a deployment that never set `WEBHOOK_SECRET` accepted forged votes
/// from anyone who read the README.
const PLACEHOLDER_SECRETS: &[&str] = &["test_secret", "XXXXXX"];

lazy_static! {
    static ref WEBHOOK_SECRET: String = check_secret(env::var("WEBHOOK_SECRET").ok())
        .unwrap_or_else(|why| panic!("refusing to start: {why}"));
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

    /// Create a new [`VotingContext`] with a given pool.
    #[allow(clippy::unused_async)]
    pub async fn new_with_pool(pool: sqlx::PgPool) -> Self {
        let secret = get_secret();
        VotingContext {
            pool: Arc::new(pool),
            secret,
        }
    }
}

/// `NewClass` for the Webhook to store in the database.
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
///
/// # Panics
/// If `WEBHOOK_SECRET` is unset, blank, or a placeholder: see [`check_secret`].
fn get_secret() -> &'static str {
    &WEBHOOK_SECRET
}

/// The webhook secret to require, or why there isn't a usable one. There is no
/// fallback: without a secret the service does not start, rather than start
/// with one everybody knows.
fn check_secret(raw: Option<String>) -> Result<String, &'static str> {
    let secret = raw.ok_or("WEBHOOK_SECRET is not set")?;
    if secret.trim().is_empty() {
        return Err("WEBHOOK_SECRET is empty");
    }
    if PLACEHOLDER_SECRETS.contains(&secret.as_str()) {
        return Err("WEBHOOK_SECRET is a placeholder from this repo; set a real one");
    }
    Ok(secret)
}

/// Compares `given` with `secret` in time that depends only on their lengths,
/// so response timing says nothing about how much of a guess was right.
fn secrets_match(given: &[u8], secret: &[u8]) -> bool {
    if given.len() != secret.len() {
        return false;
    }
    let diff = given
        .iter()
        .zip(secret)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    std::hint::black_box(diff) == 0
}

/// Convert the webhook type to a string.
fn webhook_type_to_string(kind: &dbl::types::WebhookType) -> String {
    match kind {
        dbl::types::WebhookType::Upvote => "upvote".to_string(),
        dbl::types::WebhookType::Test => "test".to_string(),
    }
}

/// Write the received webhook to the database.
#[allow(clippy::cast_possible_wrap)]
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
        eprintln!("Failed to insert / update user: {e}");
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
        webhook_type_to_string(&webhook.kind) as _,
        webhook.is_weekend,
        webhook.query,
    )
    .execute(ctx.pool.as_ref())
    .await;
    match res {
        Ok(_) => println!("Webhook written to database"),
        Err(e) => {
            eprintln!("Failed to write webhook to database: {e}");
            return Err(e);
        },
    }
    Ok(())
}

/// Create a filter that checks the `Authorization` header against the secret.
fn header(secret: &str) -> impl Filter<Extract = (), Error = Rejection> + Clone + '_ {
    warp::header::<String>("authorization")
        .and_then(move |val: String| async move {
            if secrets_match(val.as_bytes(), secret.as_bytes()) {
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
#[allow(clippy::unused_async)]
async fn get_webhook(
    ctx: VotingContext,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let secret = ctx.secret;
    let context = warp::any()
        .and(log_headers())
        //.and(log_body())
        .map(move || ctx.clone());

    warp::post()
        .and(path!("dbl" / "webhook"))
        .and(header(secret))
        .and(warp::body::content_length_limit(MAX_BODY_BYTES))
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
    let log = warp::log("crack-voting");
    webhook.or(health).with(log)
}

/// Run the server.
pub async fn run() {
    //-> Result<(), Box<dyn std::error::Error>> {
    // Checked before anything else, so a missing secret stops the service with
    // a clear reason instead of it serving (or waiting on the database) first.
    if let Err(why) = check_secret(env::var("WEBHOOK_SECRET").ok()) {
        eprintln!("crack-voting: refusing to start: {why}");
        std::process::exit(1);
    }
    let ctx = VotingContext::new().await; //Box::leak(Box::new(VotingContext::new().await));
    let app = get_app(ctx).await;

    warp::serve(app).run(([0, 0, 0, 0], 3030)).await;
}

/// Custom error handling for the server.
async fn custom_error(err: Rejection) -> Result<impl Reply, Rejection> {
    eprintln!("Error: {err:?}");
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

fn log_headers() -> impl Filter<Extract = (), Error = Infallible> + Copy {
    warp::header::headers_cloned()
        .map(|headers: HeaderMap| {
            for (k, v) in &headers {
                println!("{}", header_log_line(k, v));
            }
        })
        .untuple_one()
}

/// One header as it is logged. 🔒 `authorization` carries the webhook secret
/// itself, so its value is never printed. A value that is not visible ASCII is
/// logged lossily; `to_str().expect(..)` used to panic the request on one.
fn header_log_line(name: &HeaderName, value: &HeaderValue) -> String {
    if name == AUTHORIZATION {
        format!("{name}: <redacted>")
    } else {
        format!("{name}: {}", String::from_utf8_lossy(value.as_bytes()))
    }
}

#[cfg(test)]
mod test {
    // Only referenced by the `#[sqlx::test(migrator = "MIGRATOR")]` tests below, which
    // are commented out. Kept so they still work when uncommented.
    #[expect(
        dead_code,
        reason = "used only by the commented-out #[sqlx::test(migrator)] tests below"
    )]
    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    use super::*;

    // #[sqlx::test(migrator = "MIGRATOR")]
    // async fn test_voting_context_creation(pool: PgPool) -> sqlx::Result<()> {
    //     let secret = "test_secret";
    //     std::env::set_var("WEBHOOK_SECRET", secret);

    //     let context = VotingContext::new_with_pool(pool).await;

    //     assert_eq!(context.secret, secret);
    //     // We can't directly compare PgPools, but we can check if it's initialized
    //     assert!(context.pool.acquire().await.is_ok());
    //     Ok(())
    // }

    // #[sqlx::test(migrator = "MIGRATOR")]
    // async fn test_bad_req(_pool: PgPool) -> sqlx::Result<()> {
    //     let ctx = VotingContext::new().await;
    //     let secret = get_secret();
    //     println!("Secret {}", secret);
    //     let app = get_app(ctx).await;

    //     let res = warp::test::request()
    //         .method("POST")
    //         .path("/dbl/webhook")
    //         .header("authorization", secret)
    //         .body("bad json")
    //         .reply(&app)
    //         .await;
    //     assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    //     Ok(())
    // }

    // #[sqlx::test(migrator = "MIGRATOR")]
    // //#[sqlx::test]
    // async fn test_authorized(pool: sqlx::PgPool) -> sqlx::Result<()> {
    //     let ctx = VotingContext::new_with_pool(pool).await;
    //     let secret = get_secret();
    //     let webhook = &Webhook {
    //         bot: dbl::types::BotId(11),
    //         user: dbl::types::UserId(31),
    //         kind: dbl::types::WebhookType::Test,
    //         is_weekend: false,
    //         query: Some("test".to_string()),
    //     };
    //     let json_str = serde_json::to_string(webhook).unwrap();
    //     println!("Secret {}", secret);
    //     println!("Webhook {}", json_str);
    //     let res = warp::test::request()
    //         .method("POST")
    //         .path("/dbl/webhook")
    //         .header("authorization", secret)
    //         .json(&Webhook {
    //             bot: dbl::types::BotId(11),
    //             user: dbl::types::UserId(1),
    //             kind: dbl::types::WebhookType::Test,
    //             is_weekend: false,
    //             query: Some("test".to_string()),
    //         })
    //         .reply(&get_app(ctx.clone()).await)
    //         .await;
    //     assert_eq!(res.status(), StatusCode::OK);
    //     Ok(())
    // }

    #[test]
    fn check_secret_refuses_missing_blank_and_published_secrets() {
        assert!(check_secret(None).is_err());
        assert!(check_secret(Some(String::new())).is_err());
        assert!(check_secret(Some("   ".into())).is_err());
        assert!(check_secret(Some("test_secret".into())).is_err());
        assert!(check_secret(Some("XXXXXX".into())).is_err());
        assert_eq!(
            check_secret(Some("a-real-secret".into())),
            Ok("a-real-secret".to_string())
        );
    }

    #[test]
    fn secrets_match_is_exact() {
        assert!(secrets_match(b"s3cret", b"s3cret"));
        assert!(!secrets_match(b"s3creT", b"s3cret"));
        assert!(!secrets_match(b"s3cre", b"s3cret"));
        assert!(!secrets_match(b"", b"s3cret"));
    }

    #[test]
    fn authorization_is_never_logged() {
        let line = header_log_line(&AUTHORIZATION, &HeaderValue::from_static("s3cret"));
        assert!(!line.contains("s3cret"), "{line}");
        let other = HeaderName::from_static("user-agent");
        assert_eq!(
            header_log_line(&other, &HeaderValue::from_static("top.gg")),
            "user-agent: top.gg"
        );
        // Not visible ASCII: logged lossily rather than panicking.
        let odd = HeaderValue::from_bytes(b"caf\xe9").unwrap();
        assert!(header_log_line(&other, &odd).starts_with("user-agent: caf"));
    }

    #[tokio::test]
    async fn header_filter_rejects_a_wrong_secret() {
        let app = warp::post()
            .and(header("s3cret"))
            .map(warp::reply)
            .recover(custom_error);
        for (given, want) in [
            ("s3cret", StatusCode::OK),
            ("test_secret", StatusCode::UNAUTHORIZED),
            ("s3cre", StatusCode::UNAUTHORIZED),
        ] {
            let res = warp::test::request()
                .method("POST")
                .header("authorization", given)
                .reply(&app)
                .await;
            assert_eq!(res.status(), want, "{given}");
        }
    }

    #[tokio::test]
    async fn oversized_bodies_are_refused() {
        let app = warp::post()
            .and(warp::body::content_length_limit(MAX_BODY_BYTES))
            .and(warp::body::json())
            .map(|_: serde_json::Value| warp::reply());
        let big = format!("\"{}\"", "a".repeat(MAX_BODY_BYTES as usize + 1));
        let res = warp::test::request()
            .method("POST")
            .body(big)
            .reply(&app)
            .await;
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[sqlx::test]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn test_log_headers() {
        let app = warp::post().and(log_headers()).map(warp::reply);
        let secret = "asdf";
        let _res = warp::test::request()
            .method("POST")
            .path("/dbl/webhook")
            .header("authorization", secret)
            .body("test body")
            .reply(&app)
            .await;
    }
}
