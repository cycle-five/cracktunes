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
(1, '🔧 Test', 1234, 'https://example.com/avatar.jpg', false, NOW(), NOW(), NOW());

CREATE TABLE IF NOT EXISTS user_votes (
    id SERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    site TEXT NOT NULL,
    CONSTRAINT crack_voting_user_id_fkey FOREIGN KEY (user_id) REFERENCES "user"(id) ON DELETE CASCADE
);

CREATE INDEX user_votes_user_id_idx ON user_votes(user_id, timestamp, site);

CREATE TABLE permission_settings (
    id SERIAL PRIMARY KEY,
    default_allow_all_commands BOOLEAN NOT NULL,
    default_allow_all_users BOOLEAN NOT NULL,
    default_allow_all_roles BOOLEAN NOT NULL,
    allowed_roles BIGINT[] NOT NULL,
    denied_roles BIGINT[] NOT NULL,
    allowed_users BIGINT[] NOT NULL,
    denied_users BIGINT[] NOT NULL,
    allowed_channels BIGINT[] NOT NULL DEFAULT array[]::BIGINT[],
    denied_channels BIGINT[] NOT NULL DEFAULT array[]::BIGINT[]
);
    -- allowed_commands JSONB NOT NULL,
    -- denied_commands JSONB NOT NULL,
CREATE TABLE IF NOT EXISTS guild (
    id BIGINT NOT NULL PRIMARY KEY,
    "name" TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO guild (id, "name", created_at, updated_at) VALUES
(1, '🔧 Test', NOW(), NOW());

CREATE TABLE command_channel (
    command TEXT NOT NULL,
    guild_id BIGINT NOT NULL,
    channel_id BIGINT NOT NULL,
    permission_settings_id BIGINT NOT NULL,
    PRIMARY KEY (command, guild_id, channel_id),
    CONSTRAINT fk_command_channel_permission_settings_id FOREIGN KEY (permission_settings_id) REFERENCES permission_settings(id),
    CONSTRAINT fk_command_channel_guild_id FOREIGN KEY (guild_id) REFERENCES guild(id)
    -- Maybe add this constraint, but then need to record all channels in each guild we're in...
    -- CONSTRAINT fk_command_channel_channel_id FOREIGN KEY (channel_id) REFERENCES channel(id)
);

-- CREATE TABLE IF NOT EXISTS "role" (
--    id BIGINT NOT NULL PRIMARY KEY,
--    name TEXT NOT NULL,
--    color BIGINT NOT NULL,
--    position SMALLINT NOT NULL,
--    permissions BIGINT NOT NULL,
--    managed BOOLEAN NOT NULL,
--    mentionable BOOLEAN NOT NULL,
--    created_at TIMESTAMP NOT NULL,
--    updated_at TIMESTAMP NOT NULL
-- );

-- Add migration script here
CREATE TABLE IF NOT EXISTS metadata (
    id SERIAL PRIMARY KEY,
    track TEXT,
    artist TEXT,
    album TEXT,
    date DATE,
    channels SMALLINT,
    channel TEXT,
    start_time BIGINT NOT NULL DEFAULT 0,
    duration BIGINT NOT NULL DEFAULT 0,
    sample_rate INTEGER,
    source_url TEXT,
    title TEXT,
    thumbnail TEXT
);