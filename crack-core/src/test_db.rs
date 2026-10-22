//! Template databases for crack-core's db tests, the runner behind
//! `#[crack_test_macros::db_test(migrator = "MIGRATOR")]`.
//!
//! 🔑 `#[sqlx::test]` creates an empty database for every test and runs every
//! migration into it, about 950 ms a test. Here the migrations run once, into a
//! template, and each test gets a `CREATE DATABASE … TEMPLATE` copy of it,
//! measured at about 25 ms.
//!
//! - **One template per set of migrations,** named by [`template_name`]. The
//!   first test that needs it builds it, under an advisory lock so parallel
//!   processes don't race, and under a temporary name that is renamed once
//!   complete, so no test ever copies a half-migrated template.
//! - **One database per test,** named by [`test_db_name`]. It is dropped when
//!   the test passes and kept when it fails, for inspection.
//! - **Old templates are swept,** keeping the newest few. Worktrees on different
//!   branches can need different templates at the same time.
//!
//! 🪤 nextest runs every test in its own process, so nothing can be remembered
//! in memory between tests. Whether the template exists is one query against
//! `pg_database`, made by every test.

use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{AssertSqlSafe, Connection, Executor, PgConnection, PgPool};
use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Every template's name starts with this, so the sweep can find them all.
const TEMPLATE_PREFIX: &str = "crack_tpl_";
/// Every test database's name starts with this.
const TEST_DB_PREFIX: &str = "crack_test_";
/// A template is built under its name plus this, and renamed once complete.
const BUILD_SUFFIX: &str = "_build";

/// The session-level advisory lock that every template build, and every check
/// for an existing template, takes: the bytes of `cracktpl`.
///
/// 🔑 One key for all templates, not one per template, because the sweep at the
/// end of a build drops *other* templates. sqlx's own harness locks `sqlxtest`,
/// so the two can't collide.
const TEMPLATE_LOCK_KEY: i64 = i64::from_be_bytes(*b"cracktpl");

/// How many templates the sweep keeps, newest first by the build time in each
/// one's comment. Enough for a few worktrees on branches with different
/// migrations; each template is a few MB.
const TEMPLATES_KEPT: usize = 3;

/// The test pool's cap, as `#[sqlx::test]` caps it.
///
/// 🔑 nextest runs one test per CPU, and the server's limit is 100
/// connections, locally and in CI.
const TEST_POOL_MAX_CONNECTIONS: u32 = 5;
/// Close idle test connections quickly, as `#[sqlx::test]` does.
const TEST_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a passing test's pool gets to close before its database is dropped
/// regardless.
const POOL_CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// SQLSTATE `object_in_use`: "database … is being accessed by other users".
const OBJECT_IN_USE: &str = "55006";
/// SQLSTATE `invalid_catalog_name`: "template database … does not exist".
const UNDEFINED_DATABASE: &str = "3D000";

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a, 64-bit, continued from `hash` over `bytes`.
///
/// 🔑 Written out rather than taken from `DefaultHasher`, whose output may
/// change between Rust releases. The name it feeds is matched against databases
/// that outlive the build, so a toolchain update must not rename them.
fn fnv1a(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

/// Whether a test's return value counts as a pass.
///
/// A passing test's database is dropped; a failing one's is kept, so it can be
/// inspected. `#[sqlx::test]` draws the same line.
pub(crate) trait Outcome {
    fn passed(&self) -> bool;
}

impl Outcome for () {
    fn passed(&self) -> bool {
        true
    }
}

impl<T, E> Outcome for Result<T, E> {
    fn passed(&self) -> bool {
        self.is_ok()
    }
}

/// The template database for this set of migrations: `crack_tpl_` and a hash of
/// every migration's version and checksum, in order.
///
/// 🔑 Editing or adding a migration changes the hash, so it gets a new
/// template. A stale one is never reused, and nothing has to be invalidated by
/// hand.
pub(crate) fn template_name(migrator: &Migrator) -> String {
    let hash = migrator
        .migrations
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, migration| {
            let hash = fnv1a(hash, &migration.version.to_le_bytes());
            fnv1a(hash, &migration.checksum)
        });
    format!("{TEMPLATE_PREFIX}{hash:016x}")
}

/// The database one test runs in: `crack_test_` and a hash of the test's path.
///
/// 🔑 The same test always gets the same name, so a re-run drops whatever a
/// failed run left behind. The hash keeps the name inside Postgres's 63-byte
/// identifier limit, however deep the module path.
pub(crate) fn test_db_name(test_path: &str) -> String {
    let hash = fnv1a(FNV_OFFSET_BASIS, test_path.as_bytes());
    format!("{TEST_DB_PREFIX}{hash:016x}")
}

/// Makes sure the migrator's template exists, building it if not, and returns
/// its name. `admin` is a connection to `DATABASE_URL`'s database.
///
/// 🔑 The lock covers the existence check as well as the build, so a test that
/// finds the template never finds it half-built, and two processes that both
/// find it missing build it once.
pub(crate) async fn ensure_template(
    admin: &mut PgConnection,
    migrator: &'static Migrator,
) -> sqlx::Result<String> {
    let name = template_name(migrator);

    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(TEMPLATE_LOCK_KEY)
        .execute(&mut *admin)
        .await?;
    let built = build_if_missing(admin, migrator, &name).await;
    // 🪤 Unlock even when the build failed. The lock belongs to the session,
    // not to a transaction, so it outlives this call along with `admin`.
    let unlocked = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(TEMPLATE_LOCK_KEY)
        .execute(&mut *admin)
        .await;

    built?;
    unlocked?;
    Ok(name)
}

/// The body of [`ensure_template`], run while it holds the lock.
async fn build_if_missing(
    admin: &mut PgConnection,
    migrator: &Migrator,
    name: &str,
) -> sqlx::Result<()> {
    if database_exists(admin, name).await? {
        return Ok(());
    }

    // A crashed build leaves its `_build` database behind.
    let build = format!("{name}{BUILD_SUFFIX}");
    drop_database(admin, &build).await?;
    admin
        .execute(AssertSqlSafe(format!("CREATE DATABASE {}", ident(&build))))
        .await?;

    let mut conn = PgConnection::connect_with(&admin_options().database(&build)).await?;
    migrator.run(&mut conn).await?;
    // 🪤 Postgres refuses to rename, or to copy, a database that anyone is
    // connected to.
    conn.close().await?;

    admin
        .execute(AssertSqlSafe(format!(
            "ALTER DATABASE {} RENAME TO {}",
            ident(&build),
            ident(name)
        )))
        .await?;
    admin
        .execute(AssertSqlSafe(format!(
            "COMMENT ON DATABASE {} IS '{}'",
            ident(name),
            unix_seconds()
        )))
        .await?;

    sweep_templates(admin, name).await
}

/// Drops the templates [`stale_templates`] picks out.
async fn sweep_templates(admin: &mut PgConnection, current: &str) -> sqlx::Result<()> {
    let templates: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT datname::text, shobj_description(oid, 'pg_database') \
         FROM pg_database WHERE starts_with(datname::text, $1)",
    )
    .bind(TEMPLATE_PREFIX)
    .fetch_all(&mut *admin)
    .await?;

    for stale in stale_templates(templates, current) {
        // Not forced: someone may be looking at it by hand. It goes in a later
        // sweep instead.
        let dropped = admin
            .execute(AssertSqlSafe(format!(
                "DROP DATABASE IF EXISTS {}",
                ident(&stale)
            )))
            .await;
        if let Err(e) = dropped {
            if !has_code(&e, OBJECT_IN_USE) {
                return Err(e);
            }
        }
    }
    Ok(())
}

/// The templates to drop: all but the [`TEMPLATES_KEPT`] newest, by the unix
/// time in each one's comment, and never `current`. A template without a
/// readable comment counts as the oldest.
fn stale_templates(mut templates: Vec<(String, Option<String>)>, current: &str) -> Vec<String> {
    templates.sort_by_key(|(_, comment)| {
        std::cmp::Reverse(comment.as_deref().and_then(|c| c.parse::<u64>().ok()))
    });
    templates
        .into_iter()
        .skip(TEMPLATES_KEPT)
        .map(|(name, _)| name)
        .filter(|name| name != current)
        .collect()
}

/// Runs one test in its own copy of the migrator's template.
///
/// The database is dropped if the test passes, and kept if it fails or panics,
/// for inspection. The next run of the same test replaces it.
pub(crate) fn run<F, Fut>(migrator: &'static Migrator, test_path: &str, test_fn: F) -> Fut::Output
where
    F: FnOnce(PgPool) -> Fut,
    Fut: Future,
    Fut::Output: Outcome,
{
    // The runtime `#[sqlx::test]` uses.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to start a Tokio runtime")
        .block_on(async move {
            let options = admin_options();
            let db_name = test_db_name(test_path);
            create_test_db(&options, migrator, &db_name).await;

            let pool = PgPoolOptions::new()
                .max_connections(TEST_POOL_MAX_CONNECTIONS)
                .idle_timeout(TEST_POOL_IDLE_TIMEOUT)
                .connect_with(options.clone().database(&db_name))
                .await
                .unwrap_or_else(|e| unreachable_server(&options, &e));

            let outcome = test_fn(pool.clone()).await;

            if outcome.passed() {
                if tokio::time::timeout(POOL_CLOSE_TIMEOUT, pool.close())
                    .await
                    .is_err()
                {
                    eprintln!("test {test_path} held onto its pool after it finished");
                }
                let mut admin = connect(&options).await;
                if let Err(e) = drop_database(&mut admin, &db_name).await {
                    eprintln!("could not drop {db_name} after {test_path} passed: {e}");
                }
                admin.close().await.ok();
            }
            outcome
        })
}

/// Replaces any database a failed run of this test left behind with a fresh
/// copy of the template.
async fn create_test_db(options: &PgConnectOptions, migrator: &'static Migrator, db_name: &str) {
    let mut admin = connect(options).await;
    drop_database(&mut admin, db_name)
        .await
        .unwrap_or_else(|e| panic!("could not drop the leftover {db_name}: {e}"));

    let template = template_for(&mut admin, migrator).await;
    if let Err(e) = copy_database(&mut admin, &template, db_name).await {
        // 🪤 A build for other migrations (another worktree's, or the racing
        // test's below) can sweep this template between the check and the
        // copy. Build it again, once.
        if !has_code(&e, UNDEFINED_DATABASE) {
            panic!("could not copy {template} to {db_name}: {e}");
        }
        let template = template_for(&mut admin, migrator).await;
        copy_database(&mut admin, &template, db_name)
            .await
            .unwrap_or_else(|e| panic!("could not copy {template} to {db_name}: {e}"));
    }

    // 🔑 Closed before the test starts, so a running test holds at most its
    // pool's connections.
    admin.close().await.ok();
}

async fn template_for(admin: &mut PgConnection, migrator: &'static Migrator) -> String {
    ensure_template(admin, migrator).await.unwrap_or_else(|e| {
        panic!(
            "could not build the test template {}: {e}",
            template_name(migrator)
        )
    })
}

async fn copy_database(admin: &mut PgConnection, template: &str, name: &str) -> sqlx::Result<()> {
    admin
        .execute(AssertSqlSafe(format!(
            "CREATE DATABASE {} TEMPLATE {}",
            ident(name),
            ident(template)
        )))
        .await
        .map(drop)
}

async fn database_exists(admin: &mut PgConnection, name: &str) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)")
        .bind(name)
        .fetch_one(admin)
        .await
}

/// Drops `name` if it exists, ending any session still connected to it.
async fn drop_database(admin: &mut PgConnection, name: &str) -> sqlx::Result<()> {
    admin
        .execute(AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            ident(name)
        )))
        .await
        .map(drop)
}

/// The server and database `DATABASE_URL` names.
fn admin_options() -> PgConnectOptions {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(std::env::VarError::NotPresent) => panic!("DATABASE_URL must be set for db-tests"),
        // 🪤 Not `expect`: the error's Debug output is the raw value, password
        // and all.
        Err(std::env::VarError::NotUnicode(_)) => panic!("DATABASE_URL is not valid UTF-8"),
    };
    url.parse()
        .unwrap_or_else(|e| panic!("DATABASE_URL is not a postgres URL: {e}"))
}

async fn connect(options: &PgConnectOptions) -> PgConnection {
    PgConnection::connect_with(options)
        .await
        .unwrap_or_else(|e| unreachable_server(options, &e))
}

/// Panics naming the server's host and port. Never the URL: it holds the
/// password.
fn unreachable_server(options: &PgConnectOptions, error: &sqlx::Error) -> ! {
    panic!(
        "could not connect to postgres at {}:{} ({error}); is postgres running?",
        options.get_host(),
        options.get_port()
    )
}

fn has_code(error: &sqlx::Error, code: &str) -> bool {
    matches!(error, sqlx::Error::Database(e) if e.code().as_deref() == Some(code))
}

/// `name` as a quoted identifier. The names here are all generated, but
/// quoting costs nothing.
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    static MIGRATOR: Migrator = sqlx::migrate!("./test_migrations");

    #[test]
    fn template_name_is_stable_and_prefixed() {
        let name = template_name(&MIGRATOR);
        assert_eq!(name, template_name(&MIGRATOR));
        assert!(name.starts_with("crack_tpl_"), "{name}");
        assert_eq!(name.len(), 26, "{name}");
    }

    #[test]
    fn template_name_changes_when_a_checksum_changes() {
        let mut migrations = MIGRATOR.migrations.to_vec();
        let last = migrations.last_mut().expect("at least one migration");
        let mut checksum = last.checksum.to_vec();
        checksum[0] ^= 1;
        last.checksum = checksum.into();
        let changed = Migrator {
            migrations: migrations.into(),
            ..Migrator::DEFAULT
        };

        assert_ne!(template_name(&MIGRATOR), template_name(&changed));
    }

    #[test]
    fn test_db_name_fits_a_postgres_identifier() {
        let long = "crack_core::a::very::long::path::".repeat(10);
        let long = &long[..300];
        let name = test_db_name(long);

        assert!(name.len() <= 63, "{name}");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "{name}"
        );
        assert!(name.starts_with("crack_test_"), "{name}");
        assert_ne!(name, test_db_name("crack_core::some::other::test"));
    }

    #[test]
    fn outcome_passed() {
        assert!(().passed());
        assert!(Ok::<(), ()>(()).passed());
        assert!(!Err::<(), ()>(()).passed());
    }

    #[test]
    fn the_sweep_keeps_the_newest_templates_and_the_current_one() {
        let template =
            |name: &str, comment: Option<&str>| (name.to_string(), comment.map(str::to_string));
        let stale = stale_templates(
            vec![
                template("old", Some("100")),
                template("unreadable", Some("yesterday")),
                template("newest", Some("500")),
                template("uncommented", None),
                template("newer", Some("400")),
                template("new", Some("300")),
                // Older than the three newest, but just built: a skewed clock.
                template("current", Some("50")),
            ],
            "current",
        );
        assert_eq!(stale, ["old", "unreadable", "uncommented"]);
    }

    // The tests below drive the runner itself, so they are plain `#[test]`s
    // that check the server directly.

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a Tokio runtime")
            .block_on(future)
    }

    async fn admin() -> PgConnection {
        connect(&admin_options()).await
    }

    async fn database_count(admin: &mut PgConnection, name: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM pg_database WHERE datname = $1")
            .bind(name)
            .fetch_one(admin)
            .await
            .expect("pg_database")
    }

    #[test]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    fn run_gives_a_migrated_database() {
        let expected = MIGRATOR
            .migrations
            .iter()
            .filter(|migration| !migration.migration_type.is_down_migration())
            .count();
        let expected = i64::try_from(expected).expect("a migration count");

        run(
            &MIGRATOR,
            concat!(module_path!(), "::run_gives_a_migrated_database"),
            move |pool| async move {
                let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
                    .fetch_one(&pool)
                    .await
                    .expect("count the applied migrations");
                assert_eq!(applied, expected);
            },
        );
    }

    #[test]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    fn run_drops_the_database_after_a_pass() {
        let path = concat!(module_path!(), "::run_drops_the_database_after_a_pass");
        let name = test_db_name(path);

        let inside = name.clone();
        run(&MIGRATOR, path, |pool| async move {
            // Proves the database existed, and that the pool was pointed at it.
            let current: String = sqlx::query_scalar("SELECT current_database()::text")
                .fetch_one(&pool)
                .await
                .expect("current_database()");
            assert_eq!(current, inside);
        });

        let left = block_on(async { database_count(&mut admin().await, &name).await });
        assert_eq!(left, 0, "{name} outlived a passing test");
    }

    #[test]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    fn run_keeps_the_database_after_a_failure() {
        let path = concat!(module_path!(), "::run_keeps_the_database_after_a_failure");
        let name = test_db_name(path);

        let outcome = run(&MIGRATOR, path, |_pool| async { Err::<(), ()>(()) });
        assert!(outcome.is_err());

        let kept = block_on(async {
            let mut admin = admin().await;
            let kept = database_count(&mut admin, &name).await;
            drop_database(&mut admin, &name).await.expect("drop it");
            kept
        });
        assert_eq!(kept, 1, "{name} was dropped after a failing test");
    }

    #[test]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    fn a_leftover_test_database_is_replaced() {
        let path = concat!(module_path!(), "::a_leftover_test_database_is_replaced");
        let name = test_db_name(path);

        block_on(async {
            let mut admin = admin().await;
            drop_database(&mut admin, &name).await.expect("drop it");
            admin
                .execute(AssertSqlSafe(format!(r#"CREATE DATABASE "{name}""#)))
                .await
                .expect("create the leftover");
            let mut leftover = PgConnection::connect_with(&admin_options().database(&name))
                .await
                .expect("connect to the leftover");
            leftover
                .execute("CREATE TABLE leftover_marker (id int)")
                .await
                .expect("mark the leftover");
            leftover.close().await.expect("close the leftover");
        });

        run(&MIGRATOR, path, |pool| async move {
            let marker: Option<String> =
                sqlx::query_scalar("SELECT to_regclass('leftover_marker')::text")
                    .fetch_one(&pool)
                    .await
                    .expect("look for the marker");
            assert_eq!(marker, None, "the test ran in the leftover database");
        });
    }

    #[test]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    fn two_runners_racing_build_one_template() {
        // A checksum no other run has used, so the template can't exist yet.
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("after 1970")
            .as_nanos()
            .to_le_bytes();
        let mut migrations = MIGRATOR.migrations.to_vec();
        migrations.last_mut().expect("a migration").checksum = unique.to_vec().into();
        let migrator: &'static Migrator = Box::leak(Box::new(Migrator {
            migrations: migrations.into(),
            ..Migrator::DEFAULT
        }));
        let name = template_name(migrator);

        let start = Barrier::new(2);
        let names: Vec<String> = std::thread::scope(|scope| {
            let racers: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        block_on(async {
                            let mut admin = admin().await;
                            start.wait();
                            let built = ensure_template(&mut admin, migrator)
                                .await
                                .expect("ensure_template");
                            admin.close().await.expect("close the admin connection");
                            built
                        })
                    })
                })
                .collect();
            racers
                .into_iter()
                .map(|racer| racer.join().expect("a racer panicked"))
                .collect()
        });

        let (templates, builds) = block_on(async {
            let mut admin = admin().await;
            let templates = database_count(&mut admin, &name).await;
            let builds = database_count(&mut admin, &format!("{name}_build")).await;
            drop_database(&mut admin, &name).await.expect("drop it");
            (templates, builds)
        });
        assert_eq!(names, [name.clone(), name.clone()]);
        assert_eq!(templates, 1, "{name}");
        assert_eq!(builds, 0, "a {name}_build database was left behind");
    }

    // The attribute, once for each return type the suite uses.

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_attribute_runs_a_test_returning_unit(pool: PgPool) {
        let one: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&pool)
            .await
            .expect("SELECT 1");
        assert_eq!(one, 1);
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_attribute_runs_a_test_returning_a_sqlx_result(
        pool: sqlx::PgPool,
    ) -> sqlx::Result<()> {
        let one: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&pool).await?;
        assert_eq!(one, 1);
        Ok(())
    }

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_attribute_runs_a_test_returning_a_cracked_result(
        pool: PgPool,
    ) -> Result<(), crate::errors::CrackedError> {
        let one: i32 = sqlx::query_scalar("SELECT 1").fetch_one(&pool).await?;
        assert_eq!(one, 1);
        Ok(())
    }

    // 🔑 The expectation sits on the outer `#[test]` fn and the unused variable
    // in the inner `async fn`, so this only builds clean under `-D warnings` if
    // lint levels reach the body.
    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    #[expect(unused_variables)]
    async fn the_attribute_carries_lint_attributes_to_the_body(pool: PgPool) {
        let unused = 1;
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("SELECT 1");
    }

    // The shape of the `macro_rules!` wrappers in `economy/lock.rs` and
    // `db/economy.rs`.
    macro_rules! wrapped_db_test {
        ($name:ident, |$pool:ident| $body:block) => {
            #[crack_test_macros::db_test(migrator = "MIGRATOR")]
            #[cfg_attr(
                not(feature = "db-tests"),
                ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
            )]
            async fn $name($pool: PgPool) $body
        };
    }

    wrapped_db_test!(the_attribute_works_inside_macro_rules, |pool| {
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .expect("SELECT 1");
    });
}
