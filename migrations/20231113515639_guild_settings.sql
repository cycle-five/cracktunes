-- Add migration script here
CREATE TABLE guild_settings (
    guild_id BIGINT NOT NULL,
    guild_name TEXT NOT NULL,
    prefix TEXT NOT NULL DEFAULT 'r!',
    premium BOOLEAN NOT NULL DEFAULT FALSE,
    autopause BOOLEAN NOT NULL DEFAULT FALSE,
    allow_all_domains BOOLEAN NOT NULL DEFAULT FALSE,
    allowed_domains TEXT [] NOT NULL DEFAULT '{}',
    banned_domains TEXT [] NOT NULL DEFAULT '{}',
    ignored_channels BIGINT [] NOT NULL DEFAULT '{}',
    old_volume FLOAT NOT NULL DEFAULT 1.0,
    volume FLOAT NOT NULL DEFAULT 1.0,
    self_deafen BOOLEAN NOT NULL DEFAULT FALSE,
    timeout_seconds INT NOT NULL DEFAULT 0,
    additional_prefixes TEXT [] NOT NULL DEFAULT '{}',
    PRIMARY KEY (guild_id)
);
CREATE TABLE welcome_settings (
    guild_id BIGINT PRIMARY KEY,
    channel_id BIGINT,
    message TEXT,
    auto_role BIGINT,
    CONSTRAINT fk_welcome_settings FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);
CREATE TABLE log_settings (
    guild_id BIGINT PRIMARY KEY,
    all_log_channel BIGINT,
    raw_event_log_channel BIGINT,
    server_log_channel BIGINT,
    member_log_channel BIGINT,
    join_leave_log_channel BIGINT,
    voice_log_channel BIGINT,
    CONSTRAINT fk_log_settings FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);
CREATE TABLE allowed_domains (
    guild_id BIGINT,
    domain TEXT,
    PRIMARY KEY (guild_id, domain),
    CONSTRAINT fk_allowed_domains FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);
CREATE TABLE banned_domains (
    guild_id BIGINT,
    domain TEXT,
    PRIMARY KEY (guild_id, domain),
    CONSTRAINT fk_banned_domains FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);
CREATE TABLE authorized_users (
    guild_id BIGINT,
    user_id BIGINT,
    permissions BIGINT,
    PRIMARY KEY (guild_id, user_id),
    CONSTRAINT fk_authorized_users FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);
CREATE TABLE ignored_channels (
    guild_id BIGINT,
    channel_id BIGINT,
    PRIMARY KEY (guild_id, channel_id),
    CONSTRAINT fk_ignored_channels FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);