-- Add migration script here
CREATE TABLE IF NOT EXISTS "user" (
    id BIGINT NOT NULL PRIMARY KEY,
    username TEXT NOT NULL,
    discriminator SMALLINT,
    avatar_url TEXT NOT NULL,
    bot BOOLEAN NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    last_seen TIMESTAMP NOT NULL
);

INSERT INTO "user" (id, username, discriminator, avatar_url, bot, created_at, updated_at, last_seen) VALUES
(1, 'test', 1234, 'https://example.com/avatar.jpg', false, NOW(), NOW(), NOW());

-- Playlists table
CREATE TABLE IF NOT EXISTS playlist (
    id SERIAL PRIMARY KEY,
    "name" TEXT NOT NULL,
    user_id BIGINT,
    privacy TEXT DEFAULT 'private' NOT NULL CHECK (privacy IN ('public', 'private', 'shared')),
    CONSTRAINT fk_playlist_user FOREIGN KEY (user_id) REFERENCES "user"(id)
);

CREATE TABLE IF NOT EXISTS playlist_track (
    id SERIAL PRIMARY KEY NOT NULL,
    playlist_id INTEGER NOT NULL,
    metadata_id INTEGER NOT NULL,
    guild_id BIGINT,
    channel_id BIGINT
);