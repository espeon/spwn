CREATE TABLE IF NOT EXISTS accounts (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    vcpu_limit    INTEGER NOT NULL DEFAULT 8,
    mem_limit_mb  INTEGER NOT NULL DEFAULT 12288,
    vm_limit      INTEGER NOT NULL DEFAULT 5,
    created_at    BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);
