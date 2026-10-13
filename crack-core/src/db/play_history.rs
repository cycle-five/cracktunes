//! Track search over a guild's play log: the dashboard's history source.
//! `play_log` is written when a `/play` path (and, from v0.27.0, a dashboard
//! add) queues a track; `/gp` never writes it, so a search here cannot reveal
//! a running game's songs.

use chrono::{DateTime, Utc};
use serenity::all::GuildId;
use sqlx::PgPool;

/// One track the guild has queued, with how often and when last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryTrack {
    pub url: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Whole seconds; `None` where the log holds 0 (unknown).
    pub duration_secs: Option<u64>,
    pub plays: u32,
    pub last: DateTime<Utc>,
}

/// `q` as an `ILIKE` pattern matching it literally anywhere: `\`, `%` and `_`
/// escaped with `\` (the default escape character), then wrapped in `%`.
#[must_use]
pub fn contains_pattern(q: &str) -> String {
    let mut out = String::with_capacity(q.len() + 2);
    out.push('%');
    for c in q.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('%');
    out
}

/// How long the history query may run in the database, in milliseconds. Matches
/// `music::search::LOCAL_TIMEOUT` (2 s): `play_log` has no guild index, and a
/// dropped sqlx future does not cancel its statement, so without this a timed-out
/// scan would keep a pooled connection busy after the member was told so.
pub(crate) const HISTORY_STATEMENT_TIMEOUT_MS: u32 = 2000;

/// `SET LOCAL` takes no bind parameter; the value is our const, not input.
async fn set_local_timeout(tx: &mut sqlx::PgConnection) -> sqlx::Result<()> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SET LOCAL statement_timeout = '{HISTORY_STATEMENT_TIMEOUT_MS}ms'"
    )))
    .execute(tx)
    .await
    .map(|_| ())
}

/// Tracks queued in `guild` whose title or artist contains `q`, one row per
/// URL, newest first. `since` bounds the window (a free server's 24 hours).
/// No member column is read: a search shows tracks, never who queued them.
///
/// 🪤 `play_log.created_at` is `TIMESTAMP` without a time zone, written as UTC
/// by `CURRENT_TIMESTAMP` on a UTC server: compare and read it as naive UTC.
pub async fn search_play_log(
    pool: &PgPool,
    guild: GuildId,
    q: &str,
    since: Option<DateTime<Utc>>,
    limit: i64,
) -> sqlx::Result<Vec<HistoryTrack>> {
    let pattern = contains_pattern(q);
    let since = since.map(|t| t.naive_utc());
    let mut tx = pool.begin().await?;
    set_local_timeout(&mut tx).await?;
    let rows = sqlx::query!(
        r#"SELECT m.source_url AS "url!",
                  (array_agg(m.title ORDER BY p.created_at DESC))[1] AS title,
                  (array_agg(m.artist ORDER BY p.created_at DESC))[1] AS artist,
                  (array_agg(m.duration ORDER BY p.created_at DESC))[1] AS "duration!",
                  count(*) AS "plays!",
                  max(p.created_at) AS "last!"
           FROM play_log p
           JOIN metadata m ON m.id = p.metadata_id
           WHERE p.guild_id = $1
             AND m.source_url IS NOT NULL
             AND ($3::timestamp IS NULL OR p.created_at >= $3)
             AND (m.title ILIKE $2 OR m.artist ILIKE $2)
           GROUP BY m.source_url
           ORDER BY max(p.created_at) DESC
           LIMIT $4"#,
        guild.get() as i64,
        pattern,
        since,
        limit,
    )
    .fetch_all(&mut *tx)
    .await?;
    // Read-only: nothing to commit, and dropping `tx` rolls it back.
    tx.rollback().await?;
    Ok(rows
        .into_iter()
        .map(|r| HistoryTrack {
            url: r.url,
            title: r.title,
            artist: r.artist,
            duration_secs: u64::try_from(r.duration).ok().filter(|&d| d > 0),
            plays: u32::try_from(r.plays).unwrap_or(u32::MAX),
            last: r.last.and_utc(),
        })
        .collect())
}

#[cfg(test)]
mod test {
    use super::*;

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    #[test]
    fn the_pattern_matches_the_text_literally() {
        assert_eq!(contains_pattern("abba"), "%abba%");
        assert_eq!(contains_pattern("100%"), "%100\\%%");
        assert_eq!(contains_pattern("a_b"), "%a\\_b%");
        assert_eq!(contains_pattern("AC\\DC"), "%AC\\\\DC%");
    }

    /// A metadata row and `plays` play_log rows for it, `mins_ago` apart.
    async fn played(
        pool: &PgPool,
        guild: i64,
        url: &str,
        title: Option<&str>,
        artist: Option<&str>,
        duration: i64,
        mins_ago: &[i64],
    ) {
        let (id,): (i32,) = sqlx::query_as(
            "INSERT INTO metadata (title, artist, source_url, duration) VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(title)
        .bind(artist)
        .bind(url)
        .bind(duration)
        .fetch_one(pool)
        .await
        .unwrap();
        for m in mins_ago {
            sqlx::query(
                "INSERT INTO play_log (user_id, guild_id, metadata_id, created_at)
                 VALUES (1, $1, $2, (now() AT TIME ZONE 'utc') - make_interval(mins => $3::int))",
            )
            .bind(guild)
            .bind(id)
            .bind(*m as i32)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    async fn second_guild(pool: &PgPool) {
        sqlx::query(
            "INSERT INTO guild_settings (guild_id, guild_name, prefix, premium, autopause,
                 allow_all_domains, allowed_domains, banned_domains, ignored_channels,
                 old_volume, volume, self_deafen, timeout_seconds, additional_prefixes)
             VALUES (2, 'two', 'r!', false, false, true, '{}', '{}', '{}', 1.0, 1.0, true, 360, '{}')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn one_row_per_track_newest_first_this_guild_only(pool: PgPool) {
        second_guild(&pool).await;
        played(
            &pool,
            1,
            "https://y/old",
            Some("Zebra Song"),
            Some("Zed"),
            200,
            &[300, 200],
        )
        .await;
        played(
            &pool,
            1,
            "https://y/new",
            Some("zebra crossing"),
            None,
            0,
            &[5],
        )
        .await;
        played(
            &pool,
            2,
            "https://y/elsewhere",
            Some("Zebra elsewhere"),
            None,
            10,
            &[1],
        )
        .await;
        played(
            &pool,
            1,
            "https://y/byartist",
            Some("Untitled"),
            Some("The Zebras"),
            30,
            &[60],
        )
        .await;
        let got = search_play_log(&pool, GuildId::new(1), "ZEBRA", None, 10)
            .await
            .unwrap();
        let urls: Vec<&str> = got.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(
            urls,
            ["https://y/new", "https://y/byartist", "https://y/old"]
        );
        assert_eq!((got[2].plays, got[2].duration_secs), (2, Some(200)));
        assert_eq!(got[0].duration_secs, None, "0 seconds is unknown");
        assert!(got[0].last > got[1].last && got[1].last > got[2].last);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_window_the_limit_and_literal_wildcards(pool: PgPool) {
        played(
            &pool,
            1,
            "https://y/recent",
            Some("100% hits"),
            None,
            1,
            &[30],
        )
        .await;
        played(
            &pool,
            1,
            "https://y/ancient",
            Some("100% old"),
            None,
            1,
            &[60 * 48],
        )
        .await;
        played(
            &pool,
            1,
            "https://y/decoy",
            Some("1000 hits"),
            None,
            1,
            &[10],
        )
        .await;
        played(
            &pool,
            1,
            "https://y/null",
            None,
            Some("Hundred% Club"),
            1,
            &[10],
        )
        .await;
        let since = Some(Utc::now() - chrono::Duration::hours(24));
        let got = search_play_log(&pool, GuildId::new(1), "100%", since, 10)
            .await
            .unwrap();
        let urls: Vec<&str> = got.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(
            urls,
            ["https://y/recent"],
            "the % is literal and the window holds"
        );
        let all = search_play_log(&pool, GuildId::new(1), "100%", None, 10)
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        let one = search_play_log(&pool, GuildId::new(1), "100%", None, 1)
            .await
            .unwrap();
        assert_eq!(one.len(), 1);
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn non_ascii_case_folds_and_a_null_title_still_matches_by_artist(pool: PgPool) {
        played(&pool, 1, "https://y/b", Some("Björk Song"), None, 1, &[1]).await;
        played(&pool, 1, "https://y/n", None, Some("BJÖRK"), 1, &[2]).await;
        let got = search_play_log(&pool, GuildId::new(1), "BJÖRK", None, 10)
            .await
            .unwrap();
        let urls: Vec<&str> = got.iter().map(|t| t.url.as_str()).collect();
        assert_eq!(urls, ["https://y/b", "https://y/n"]);
    }

    /// The query runs under the statement timeout (Ruling 5): `SET LOCAL`
    /// reaches the same connection the query uses, and ends with the transaction.
    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn the_statement_timeout_is_local_to_the_search_transaction(pool: PgPool) {
        let mut tx = pool.begin().await.unwrap();
        set_local_timeout(&mut tx).await.unwrap();
        let (t,): (String,) = sqlx::query_as("SHOW statement_timeout")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(t, "2s");
        tx.rollback().await.unwrap();
        // Back on the pool: no leftover timeout.
        search_play_log(&pool, GuildId::new(1), "x", None, 1)
            .await
            .unwrap();
        let (t,): (String,) = sqlx::query_as("SHOW statement_timeout")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(t, "0");
    }
}
