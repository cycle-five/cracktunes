//! The server mutes CrackTunes put on members because a room muted them, kept
//! until undone. A hold outlives the room's own row, which goes when the room
//! empties. See `rooms::mute_hold`.

use serenity::all::{ChannelId, GuildId, UserId};
use sqlx::PgPool;

/// A stored id as a serenity id; `None` for zero or negative, which no
/// snowflake is (`new(0)` would panic).
fn positive(id: i64) -> Option<u64> {
    u64::try_from(id).ok().filter(|v| *v != 0)
}

/// The room whose mute is held on `u`, if any.
pub async fn held_for(pool: &PgPool, g: GuildId, u: UserId) -> sqlx::Result<Option<ChannelId>> {
    let row = sqlx::query!(
        r#"SELECT channel_id FROM room_mute_holds WHERE guild_id = $1 AND user_id = $2"#,
        g.get() as i64,
        u.get() as i64,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|r| positive(r.channel_id)).map(ChannelId::new))
}

/// Record that CrackTunes holds `u` muted for room `ch`; an existing hold
/// moves to `ch`.
pub async fn hold(pool: &PgPool, g: GuildId, u: UserId, ch: ChannelId) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO room_mute_holds (guild_id, user_id, channel_id)
           VALUES ($1, $2, $3)
           ON CONFLICT (guild_id, user_id) DO UPDATE SET channel_id = EXCLUDED.channel_id"#,
        g.get() as i64,
        u.get() as i64,
        ch.get() as i64,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Forget the hold on `u`. Whether there was one.
pub async fn release(pool: &PgPool, g: GuildId, u: UserId) -> sqlx::Result<bool> {
    let done = sqlx::query!(
        r#"DELETE FROM room_mute_holds WHERE guild_id = $1 AND user_id = $2"#,
        g.get() as i64,
        u.get() as i64,
    )
    .execute(pool)
    .await?;
    Ok(done.rows_affected() > 0)
}

/// Every hold in the server, with the room it is for.
pub async fn holds_in(pool: &PgPool, g: GuildId) -> sqlx::Result<Vec<(UserId, ChannelId)>> {
    let rows = sqlx::query!(
        r#"SELECT user_id, channel_id FROM room_mute_holds WHERE guild_id = $1
           ORDER BY user_id"#,
        g.get() as i64,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some((
                UserId::new(positive(r.user_id)?),
                ChannelId::new(positive(r.channel_id)?),
            ))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../crack-core/test_migrations");

    const G: GuildId = GuildId::new(1);
    const T: UserId = UserId::new(7);

    #[crack_test_macros::db_test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_hold_is_kept_moved_listed_and_released(pool: PgPool) -> sqlx::Result<()> {
        assert_eq!(held_for(&pool, G, T).await?, None);
        hold(&pool, G, T, ChannelId::new(10)).await?;
        assert_eq!(held_for(&pool, G, T).await?, Some(ChannelId::new(10)));
        hold(&pool, G, T, ChannelId::new(11)).await?;
        hold(&pool, GuildId::new(2), T, ChannelId::new(20)).await?;
        assert_eq!(
            holds_in(&pool, G).await?,
            vec![(T, ChannelId::new(11))],
            "one row per member per server, and other servers' holds stay out"
        );
        assert!(release(&pool, G, T).await?);
        assert!(!release(&pool, G, T).await?);
        assert_eq!(held_for(&pool, G, T).await?, None);
        assert_eq!(
            held_for(&pool, GuildId::new(2), T).await?,
            Some(ChannelId::new(20))
        );
        Ok(())
    }
}
