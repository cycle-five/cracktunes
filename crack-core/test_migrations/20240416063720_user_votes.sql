-- Table to track top.gg votes for the bot.

CREATE TABLE user_votes (
    id SERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    site TEXT NOT NULL,
    CONSTRAINT crack_voting_user_id_fkey FOREIGN KEY (user_id) REFERENCES "user"(id) ON DELETE CASCADE
);

CREATE INDEX user_votes_user_id_idx ON user_votes(user_id, timestamp, site);