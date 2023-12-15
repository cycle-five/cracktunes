-- Add migration script here
CREATE TABLE IF NOT EXISTS metadata (
    id SERIAL PRIMARY KEY,
    track TEXT,
    artist TEXT,
    album TEXT,
    date DATE,
    channels SMALLINT,
    channel TEXT,
    start_time BIGINT NOT NULL DEFAULT 0,
    duration BIGINT NOT NULL DEFAULT 0,
    sample_rate INTEGER,
    source_url TEXT,
    title TEXT,
    thumbnail TEXT
);