ALTER TABLE accounts
    ALTER COLUMN password_hash DROP NOT NULL,
    ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'password';

CREATE TABLE passkeys (
    id            TEXT PRIMARY KEY,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    credential_id BYTEA NOT NULL UNIQUE,
    passkey_json  TEXT NOT NULL,
    name          TEXT NOT NULL,
    created_at    BIGINT NOT NULL
);

CREATE TABLE passkey_challenges (
    id             TEXT PRIMARY KEY,
    account_id     TEXT,
    challenge_json TEXT NOT NULL,
    kind           TEXT NOT NULL,
    expires_at     BIGINT NOT NULL
);

CREATE TABLE pending_auth_tokens (
    id         TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at BIGINT NOT NULL
);
