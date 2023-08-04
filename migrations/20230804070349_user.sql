-- Add migration script here
CREATE TABLE IF NOT EXISTS user
(
    id              INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    discord_id      TEXT    NOT NULL,
    username        TEXT    NOT NULL,
    descriminator   TEXT    NOT NULL,
    last_seen       DATE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    creation_date   DATE NOT NULL DEFAULT CURRENT_TIMESTAMP
)