-- Mod log, per server settings: the channel where the bot posts who
-- server-muted, deafened, disconnected or moved whom, and every room kick or
-- mute. Its own table (like room_settings) rather than columns on
-- guild_settings, which is saved whole from memory. Off by default; enabling
-- needs a channel.
CREATE TABLE IF NOT EXISTS mod_log_settings (
    guild_id   BIGINT PRIMARY KEY,
    enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    channel_id BIGINT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
