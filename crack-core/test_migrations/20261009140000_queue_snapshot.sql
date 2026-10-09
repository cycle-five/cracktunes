-- The music queue of every guild playing when the bot shut down, so a deploy
-- or clean reboot can pick it back up (#595). Written at shutdown only, and
-- claimed -- deleted -- by the resume, so a row comes back at most once.
CREATE TABLE IF NOT EXISTS queue_snapshot (
    guild_id          BIGINT PRIMARY KEY,
    saved_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    voice_channel_id  BIGINT NOT NULL,
    text_channel_id   BIGINT,
    status_channel_id BIGINT,
    status_message_id BIGINT,
    position_ms       BIGINT NOT NULL,
    paused            BOOLEAN NOT NULL,
    looping           BOOLEAN NOT NULL,
    autoplay          BOOLEAN NOT NULL,
    -- Vec<SnapshotTrack>, current track first (crack-core db::queue_snapshot).
    tracks            JSONB NOT NULL
);
