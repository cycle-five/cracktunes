-- Add migration script here
CREATE TABLE IF NOT EXISTS play_log (
    id SERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    guild_id BIGINT NOT NULL,
    metadata_id BIGINT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT play_log_fk_constraint FOREIGN KEY (user_id) REFERENCES "user"(id),
    FOREIGN KEY (metadata_id) REFERENCES metadata(id),
    FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
);