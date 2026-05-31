CREATE TABLE audit_log (
    id          BIGSERIAL PRIMARY KEY,
    account_id  TEXT        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    action      TEXT        NOT NULL,
    resource    TEXT        NOT NULL,
    resource_id TEXT,
    detail      JSONB,
    ip          TEXT,
    created_at  BIGINT      NOT NULL
);

CREATE INDEX audit_log_account_id ON audit_log(account_id);
CREATE INDEX audit_log_created_at ON audit_log(created_at);
