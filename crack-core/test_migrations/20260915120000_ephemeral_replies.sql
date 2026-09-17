-- Whether /play, /skip and /nowplaying reply ephemerally in this guild. Off
-- keeps visible replies, the behaviour before v0.13.0. See
-- docs/superpowers/specs/2026-09-15-floating-status-message-design.md.
ALTER TABLE guild_settings
    ADD COLUMN IF NOT EXISTS ephemeral_replies BOOLEAN NOT NULL DEFAULT FALSE;
