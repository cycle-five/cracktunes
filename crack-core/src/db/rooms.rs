//! Temporary voice rooms as rows: per-server settings and one row per live
//! room. See `rooms`.

use crate::rooms::settings::RoomSettings;
use crate::rooms::state::{RoomState, StoredOverwrite};
use serenity::all::{ChannelId, GuildId, UserId};
use sqlx::{types::Json, PgPool};

/// A live room as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRoom {
    pub channel: ChannelId,
    pub guild: GuildId,
    pub owner: UserId,
    pub state: RoomState,
    /// The overwrites Discord gave the channel at creation.
    pub baseline: Vec<StoredOverwrite>,
}

/// A stored id as a serenity id; `None` for zero or negative, which no
/// snowflake is (`new(0)` would panic).
fn positive(id: i64) -> Option<u64> {
    u64::try_from(id).ok().filter(|v| *v != 0)
}

pub async fn load_settings(pool: &PgPool, guild: GuildId) -> sqlx::Result<Option<RoomSettings>> {
    let row = sqlx::query!(
        r#"SELECT enabled, hub_channel_id, name_template, default_limit, empty_grace_seconds
           FROM room_settings WHERE guild_id = $1"#,
        guild.get() as i64
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| RoomSettings {
        guild,
        enabled: r.enabled,
        hub: r.hub_channel_id.and_then(positive).map(ChannelId::new),
        name_template: r.name_template,
        default_limit: u8::try_from(r.default_limit).unwrap_or(0),
        empty_grace_secs: u16::try_from(r.empty_grace_seconds).unwrap_or(30),
    }))
}

pub async fn save_settings(pool: &PgPool, s: &RoomSettings) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO room_settings (
               guild_id, enabled, hub_channel_id, name_template, default_limit,
               empty_grace_seconds
           )
           VALUES ($1, $2, $3, $4, $5, $6)
           ON CONFLICT (guild_id) DO UPDATE SET
               enabled = EXCLUDED.enabled,
               hub_channel_id = EXCLUDED.hub_channel_id,
               name_template = EXCLUDED.name_template,
               default_limit = EXCLUDED.default_limit,
               empty_grace_seconds = EXCLUDED.empty_grace_seconds,
               updated_at = now()"#,
        s.guild.get() as i64,
        s.enabled,
        s.hub.map(|c| c.get() as i64),
        s.name_template,
        i16::from(s.default_limit),
        i32::from(s.empty_grace_secs),
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_room(pool: &PgPool, r: &StoredRoom) -> sqlx::Result<()> {
    sqlx::query!(
        r#"INSERT INTO rooms (channel_id, guild_id, owner_id, state, baseline)
           VALUES ($1, $2, $3, $4, $5)"#,
        r.channel.get() as i64,
        r.guild.get() as i64,
        r.owner.get() as i64,
        Json(&r.state) as _,
        Json(&r.baseline) as _,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_state(
    pool: &PgPool,
    channel: ChannelId,
    owner: UserId,
    state: &RoomState,
) -> sqlx::Result<()> {
    sqlx::query!(
        "UPDATE rooms SET owner_id = $2, state = $3 WHERE channel_id = $1",
        channel.get() as i64,
        owner.get() as i64,
        Json(state) as _,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_room(pool: &PgPool, channel: ChannelId) -> sqlx::Result<()> {
    sqlx::query!(
        "DELETE FROM rooms WHERE channel_id = $1",
        channel.get() as i64
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Every stored room of a guild, oldest first. A row with an unusable id is
/// logged and skipped.
pub async fn rooms_in(pool: &PgPool, guild: GuildId) -> sqlx::Result<Vec<StoredRoom>> {
    let rows = sqlx::query!(
        r#"SELECT channel_id, owner_id,
                  state AS "state!: Json<RoomState>",
                  baseline AS "baseline!: Json<Vec<StoredOverwrite>>"
           FROM rooms WHERE guild_id = $1 ORDER BY created_at, channel_id"#,
        guild.get() as i64
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        match (positive(r.channel_id), positive(r.owner_id)) {
            (Some(channel), Some(owner)) => out.push(StoredRoom {
                channel: ChannelId::new(channel),
                guild,
                owner: UserId::new(owner),
                state: r.state.0,
                baseline: r.baseline.0,
            }),
            _ => tracing::warn!(
                channel_id = r.channel_id,
                owner_id = r.owner_id,
                "skipping a stored room with an unusable id"
            ),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rooms::state::OverwriteTarget;
    use serenity::all::{Permissions, RoleId};

    pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./test_migrations");

    fn room(channel: u64, guild: u64, owner: u64) -> StoredRoom {
        let mut state = RoomState::new(UserId::new(owner));
        state.helpers.insert(UserId::new(2));
        state.blocked.insert(UserId::new(3));
        state.muted.insert(UserId::new(4));
        state.permitted_users.insert(UserId::new(5));
        state.permitted_roles.insert(RoleId::new(6));
        state.blocked_roles.insert(RoleId::new(7));
        state.locked = true;
        state.chat_locked = true;
        state.panel = Some(serenity::all::MessageId::new(8));
        StoredRoom {
            channel: ChannelId::new(channel),
            guild: GuildId::new(guild),
            owner: UserId::new(owner),
            state,
            baseline: vec![
                StoredOverwrite {
                    target: OverwriteTarget::Role {
                        id: RoleId::new(guild),
                    },
                    allow: Permissions::VIEW_CHANNEL,
                    deny: Permissions::empty(),
                },
                StoredOverwrite {
                    target: OverwriteTarget::Member {
                        id: UserId::new(owner),
                    },
                    allow: Permissions::CONNECT | Permissions::SPEAK,
                    deny: Permissions::SEND_MESSAGES,
                },
            ],
        }
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn settings_survive_a_save_and_load(pool: PgPool) -> sqlx::Result<()> {
        let s = RoomSettings {
            guild: GuildId::new(1),
            enabled: true,
            hub: Some(ChannelId::new(55)),
            name_template: "Team {name}".to_string(),
            default_limit: 7,
            empty_grace_secs: 120,
        };
        save_settings(&pool, &s).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(s.clone()));
        let off = RoomSettings::off(s.guild);
        save_settings(&pool, &off).await?;
        assert_eq!(load_settings(&pool, s.guild).await?, Some(off), "upsert");
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn missing_settings_load_as_none(pool: PgPool) -> sqlx::Result<()> {
        assert_eq!(load_settings(&pool, GuildId::new(9)).await?, None);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn a_room_survives_insert_save_state_and_load(pool: PgPool) -> sqlx::Result<()> {
        let mut r = room(10, 1, 100);
        insert_room(&pool, &r).await?;
        assert_eq!(rooms_in(&pool, r.guild).await?, vec![r.clone()]);
        r.owner = UserId::new(200);
        r.state.owner = r.owner;
        r.state.locked = false;
        r.state.helpers.clear();
        save_state(&pool, r.channel, r.owner, &r.state).await?;
        assert_eq!(rooms_in(&pool, r.guild).await?, vec![r]);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn delete_room_removes_only_that_room(pool: PgPool) -> sqlx::Result<()> {
        insert_room(&pool, &room(10, 1, 100)).await?;
        insert_room(&pool, &room(11, 1, 101)).await?;
        delete_room(&pool, ChannelId::new(10)).await?;
        let left = rooms_in(&pool, GuildId::new(1)).await?;
        assert_eq!(left, vec![room(11, 1, 101)]);
        Ok(())
    }

    #[sqlx::test(migrator = "MIGRATOR")]
    #[cfg_attr(
        not(feature = "db-tests"),
        ignore = "needs a postgres at DATABASE_URL; enable the db-tests feature"
    )]
    async fn rooms_in_returns_only_that_guilds_rooms(pool: PgPool) -> sqlx::Result<()> {
        insert_room(&pool, &room(10, 1, 100)).await?;
        insert_room(&pool, &room(20, 2, 100)).await?;
        assert_eq!(
            rooms_in(&pool, GuildId::new(2)).await?,
            vec![room(20, 2, 100)]
        );
        assert!(rooms_in(&pool, GuildId::new(3)).await?.is_empty());
        Ok(())
    }
}
