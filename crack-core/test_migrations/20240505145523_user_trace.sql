-- Add migration script here
-- up
CREATE TABLE IF NOT EXISTS user_trace (
    user_id BIGINT NOT NULL,
    ts TIMESTAMP NOT NULL,
    whence TEXT,
    primary key (user_id, ts)
);