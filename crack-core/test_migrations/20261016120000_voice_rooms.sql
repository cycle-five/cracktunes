-- Temporary voice rooms, per server settings. Its own table rather than
-- columns on guild_settings: that row is saved whole from memory, so a second
-- writer would be clobbered. Off by default; enabling needs a hub channel.
CREATE TABLE IF NOT EXISTS room_settings (
    guild_id            BIGINT PRIMARY KEY,
    enabled             BOOLEAN NOT NULL DEFAULT false,
    -- The voice channel a member joins to get a room of their own.
    hub_channel_id      BIGINT,
    -- {name} is the owner's display name.
    name_template       TEXT NOT NULL DEFAULT '{name}''s room',
    -- 0 means no limit.
    default_limit       SMALLINT NOT NULL DEFAULT 0 CHECK (default_limit BETWEEN 0 AND 99),
    empty_grace_seconds INTEGER NOT NULL DEFAULT 30 CHECK (empty_grace_seconds IN (0, 10, 30, 60, 120, 300)),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row per live temporary voice room; deleted with the room.
CREATE TABLE IF NOT EXISTS rooms (
    channel_id BIGINT PRIMARY KEY,
    guild_id   BIGINT NOT NULL,
    owner_id   BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- RoomState (crack-core rooms::state).
    state      JSONB NOT NULL,
    -- Vec<StoredOverwrite>: the overwrites Discord gave the channel at creation.
    baseline   JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS rooms_guild_id ON rooms (guild_id);
