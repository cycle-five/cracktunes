-- Add migration script here
CREATE TABLE IF NOT EXISTS todo (
    id SERIAL PRIMARY KEY NOT NULL,
    user_id BIGINT NOT NULL,
    description TEXT NOT NULL,
    done BOOLEAN NOT NULL DEFAULT FALSE,
    creation_date DATE NOT NULL DEFAULT CURRENT_DATE,
    done_date DATE,
    CONSTRAINT fk_todo_user FOREIGN KEY (user_id) REFERENCES "user"(id)
);