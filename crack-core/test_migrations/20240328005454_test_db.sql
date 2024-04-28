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
    allowed_commands JSONB NOT NULL,
    denied_commands JSONB NOT NULL,
    allowed_roles BIGINT[] NOT NULL,
    denied_roles BIGINT[] NOT NULL,
    allowed_users BIGINT[] NOT NULL,
    denied_users BIGINT[] NOT NULL
);