-- Add migration script here
CREATE TABLE IF NOT EXISTS metadata (
    id          INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    track       TEXT,
    artist      TEXT,
    album       TEXT,
    date        DATE,
    channels    INTEGER,
    channel     TEXT,
    start_time  INTEGER,
    duration    INTEGER,
    sample_rate INTEGER,
    source_url  TEXT,
    title       TEXT,
    thumbnail   TEXT
);
