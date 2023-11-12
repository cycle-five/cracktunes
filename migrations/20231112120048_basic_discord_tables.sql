-- Add migration script here
-- Table for storing guilds (servers)
CREATE TABLE guild (
    id BIGINT PRIMARY KEY,
    name VARCHAR(255),
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);

-- Table for storing users
CREATE TABLE user (
    id BIGINT PRIMARY KEY,
    username VARCHAR(255),
    discriminator SMALLINT,
    avatar_url TEXT,
    bot BOOLEAN,
    created_at TIMESTAMP,
    updated_at TIMESTAMP
);

-- Table for storing channels
CREATE TABLE channel (
    id BIGINT PRIMARY KEY,
    guild_id BIGINT,
    name VARCHAR(255),
    type VARCHAR(50),
    position INT,
    created_at TIMESTAMP,
    updated_at TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guild(id)
);

-- Table for storing messages
CREATE TABLE message (
    id BIGINT PRIMARY KEY,
    channel_id BIGINT,
    author_id BIGINT,
    content TEXT,
    created_at TIMESTAMP,
    updated_at TIMESTAMP,
    FOREIGN KEY (channel_id) REFERENCES channel(id),
    FOREIGN KEY (author_id) REFERENCES user(id)
);

-- Table for storing roles
CREATE TABLE role (
    id BIGINT PRIMARY KEY,
    guild_id BIGINT,
    name VARCHAR(255),
    color INT,
    hoist BOOLEAN,
    position INT,
    permissions BIGINT,
    managed BOOLEAN,
    mentionable BOOLEAN,
    created_at TIMESTAMP,
    updated_at TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guild(id)
);

-- Table for storing user roles
CREATE TABLE user_role (
    user_id BIGINT,
    role_id BIGINT,
    PRIMARY KEY (user_id, role_id)
    FOREIGN KEY (user_id) REFERENCES user(id),
    FOREIGN KEY (role_id) REFERENCES role(id)
);

-- Table for storing the many-to-many relationship between users and guilds
CREATE TABLE guild_member (
    guild_id BIGINT,
    user_id BIGINT,
    nick VARCHAR(255),
    joined_at TIMESTAMP,
    PRIMARY KEY (guild_id, user_id),
    FOREIGN KEY (guild_id) REFERENCES guild(id),
    FOREIGN KEY (user_id) REFERENCES user(id)
);

CREATE TABLE user_channel (
    user_id BIGINT,
    channel_id BIGINT,
    PRIMARY KEY (user_id, channel_id),
    FOREIGN KEY (user_id) REFERENCES user(user_id),
    FOREIGN KEY (channel_id) REFERENCES channel(channel_id)
);