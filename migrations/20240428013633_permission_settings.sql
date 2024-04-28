-- Add migration script here
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

CREATE TABLE command_channel (
    id SERIAL PRIMARY KEY,
    channel_id BIGINT NOT NULL,
    permission_settings_id BIGINT NOT NULL,
    CONSTRAINT fk_command_channel_permission_settings_id FOREIGN KEY (permission_settings_id) REFERENCES permission_settings(id)
);