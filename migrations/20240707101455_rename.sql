-- Add migration script here
ALTER TABLE IF EXISTS VOTE_WEBHOOK
    RENAME TO vote_webhook;