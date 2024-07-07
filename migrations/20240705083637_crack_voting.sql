-- Add migration script here
CREATE TYPE WEBHOOK_KIND AS ENUM('upvote', 'test');
CREATE TABLE IF NOT EXISTS vote_webhook (
    id SERIAL PRIMARY KEY,
    bot_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    kind WEBHOOK_KIND NOT NULL,
    is_weekend BOOLEAN NOT NULL,
    query TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT fk_vote_webhook_user_id FOREIGN KEY (user_id) REFERENCES "user"(id)
);