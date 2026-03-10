CREATE TABLE cli_auth_codes (
    code       TEXT PRIMARY KEY,
    account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
    status     TEXT NOT NULL DEFAULT 'pending',
    expires_at BIGINT NOT NULL
);

CREATE TABLE api_tokens (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL,
    created_at   BIGINT NOT NULL,
    last_used_at BIGINT
);
