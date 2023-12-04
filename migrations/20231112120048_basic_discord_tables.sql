-- Add migration script here
-- Table for storing guilds (servers)
CREATE TABLE IF NOT EXISTS guild (
    id BIGINT NOT NULL PRIMARY KEY,
    "name" TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
-- Table for storing users
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
-- Table for storing channels
CREATE TABLE IF NOT EXISTS channel (
    id BIGINT NOT NULL PRIMARY KEY,
    guild_id BIGINT,
    name TEXT,
    type TEXT,
    position INT,
    created_at TIMESTAMP,
    updated_at TIMESTAMP,
    CONSTRAINT fk_channel FOREIGN KEY (guild_id) REFERENCES guild(id)
);
-- Table for storing messages
CREATE TABLE IF NOT EXISTS message (
    id BIGINT NOT NULL PRIMARY KEY,
    channel_id BIGINT,
    author_id BIGINT,
    content TEXT,
    created_at TIMESTAMP,
    updated_at TIMESTAMP,
    CONSTRAINT fk_message FOREIGN KEY (channel_id) REFERENCES channel(id),
    FOREIGN KEY (author_id) REFERENCES "user"(id)
);
-- Table for storing roles
CREATE TABLE IF NOT EXISTS role (
    id BIGINT NOT NULL PRIMARY KEY,
    guild_id BIGINT,
    name TEXT,
    color INT,
    hoist BOOLEAN,
    position INT,
    permissions BIGINT,
    managed BOOLEAN,
    mentionable BOOLEAN,
    created_at TIMESTAMP,
    updated_at TIMESTAMP,
    CONSTRAINT fk_role FOREIGN KEY (guild_id) REFERENCES guild(id)
);
-- Table for storing user roles
CREATE TABLE IF NOT EXISTS user_role (
    user_id BIGINT,
    role_id BIGINT,
    PRIMARY KEY (user_id, role_id),
    CONSTRAINT fk_user_role FOREIGN KEY (user_id) REFERENCES "user"(id),
    FOREIGN KEY (role_id) REFERENCES role(id)
);
-- Table for storing the many-to-many relationship between users and guilds
CREATE TABLE IF NOT EXISTS guild_member (
    guild_id BIGINT,
    user_id BIGINT,
    nick TEXT,
    joined_at TIMESTAMP,
    PRIMARY KEY (guild_id, user_id),
    CONSTRAINT fk_guild_member FOREIGN KEY (guild_id) REFERENCES guild(id),
    FOREIGN KEY (user_id) REFERENCES "user"(id)
);
CREATE TABLE user_channel (
    user_id BIGINT,
    channel_id BIGINT,
    PRIMARY KEY (user_id, channel_id),
    CONSTRAINT fk_user_channel FOREIGN KEY (user_id) REFERENCES "user"(id),
    FOREIGN KEY (channel_id) REFERENCES channel(id)
);