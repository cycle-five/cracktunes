-- A server mute CrackTunes put on a member because a room muted them, kept
-- until it is undone: it outlives the room's row, which goes when the room empties.
CREATE TABLE IF NOT EXISTS room_mute_holds (
    guild_id   BIGINT NOT NULL,
    user_id    BIGINT NOT NULL,
    channel_id BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, user_id)
);
