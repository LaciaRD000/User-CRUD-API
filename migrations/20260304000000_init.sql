-- Initial schema for user-api.

CREATE TABLE IF NOT EXISTS users (
    id            BIGINT PRIMARY KEY,
    username      TEXT NOT NULL,
    email         TEXT NOT NULL,
    password_hash TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id         BIGINT PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Ensure case-insensitive uniqueness for email.
-- If the DB previously had a case-sensitive unique constraint, drop it first.
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_email_key;
CREATE UNIQUE INDEX IF NOT EXISTS users_email_lower_key ON users (lower(email));

