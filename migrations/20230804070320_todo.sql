-- Add migration script here
CREATE TABLE IF NOT EXISTS todo
(
    id              INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    user_id         INTEGER NOT NULL,
    description     TEXT    NOT NULL,
    done            BOOLEAN NOT NULL DEFAULT FALSE,
    creation_date   DATE    NOT NULL DEFAULT CURRENT_DATE,
    done_date       DATE
);