-- The queue audit log: every change to a guild's queue, who made it and how.
-- Spec: docs/superpowers/specs/2026-09-30-queue-audit-log-design.md
-- No foreign keys, on purpose: a missing user or guild_settings row must never
-- fail an insert.
CREATE TABLE IF NOT EXISTS queue_audit (
    id                BIGSERIAL PRIMARY KEY,
    at                TIMESTAMPTZ NOT NULL,
    guild_id          BIGINT NOT NULL,
    voice_channel_id  BIGINT,
    origin_channel_id BIGINT,
    actor_user_id     BIGINT,
    source            TEXT NOT NULL,
    command           TEXT NOT NULL,
    action            TEXT NOT NULL,
    detail            JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS queue_audit_guild_at ON queue_audit (guild_id, at DESC);
CREATE INDEX IF NOT EXISTS queue_audit_actor_at ON queue_audit (actor_user_id, at DESC);
