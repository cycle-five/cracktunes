-- Add migration script here

ALTER TABLE permission_settings
    ADD COLUMN allowed_channels BIGINT[] NOT NULL DEFAULT array[]::BIGINT[],
    ADD COLUMN denied_channels BIGINT[] NOT NULL DEFAULT array[]::BIGINT[];

