-- Add user and group permissions (per guild)
CREATE TABLE IF NOT EXISTS user_permission (
    id SERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    guild_id BIGINT NOT NULL,
    permission_key TEXT NOT NULL,
    permission_value INT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_user_guild_id FOREIGN KEY (user_id) REFERENCES "user"(id),
    FOREIGN KEY (guild_id) REFERENCES guild (id)
);
CREATE TABLE IF NOT EXISTS group_permission (
    id SERIAL PRIMARY KEY,
    group_id BIGINT NOT NULL,
    guild_id BIGINT NOT NULL,
    permission_key TEXT NOT NULL,
    permission_value INT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_guild_id FOREIGN KEY (guild_id) REFERENCES guild(id)
);
-- TODO: Add table for groups in a guild