-- Add migration script here
CREATE TABLE welcome_settings (
    id SERIAL PRIMARY KEY,
    channel_id BIGINT,
    message TEXT,
    auto_role BIGINT
);
CREATE TABLE log_settings (
    id SERIAL PRIMARY KEY,
    all_log_channel BIGINT,
    raw_event_log_channel BIGINT,
    server_log_channel BIGINT,
    member_log_channel BIGINT,
    join_leave_log_channel BIGINT,
    voice_log_channel BIGINT
);
CREATE TABLE guild_settings (
    guild_id BIGINT,
    guild_name TEXT,
    prefix TEXT,
    prefix_up TEXT,
    autopause BOOLEAN,
    allow_all_domains BOOLEAN,
    old_volume FLOAT,
    volume FLOAT,
    self_deafen BOOLEAN,
    timeout_seconds INT,
    welcome_settings_id BIGINT,
    log_settings_id BIGINT,
    PRIMARY KEY (guild_id),
    CONSTRAINT fk_guild_settings FOREIGN KEY (guild_id) REFERENCES guild(id),
    FOREIGN KEY (welcome_settings_id) REFERENCES welcome_settings(id),
    FOREIGN KEY (log_settings_id) REFERENCES log_settings(id)
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
    PRIMARY KEY (guild_id, user_id),
    CONSTRAINT fk_authorized_users FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);
CREATE TABLE ignored_channels (
    guild_id BIGINT,
    channel_id BIGINT,
    PRIMARY KEY (guild_id, channel_id),
    CONSTRAINT fk_ignored_channels FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);