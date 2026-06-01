-- Short-lived auth tokens for cross-domain VM access.
-- When a user logs in via auth.spwn.town, a token is generated and embedded
-- in the redirect URL. The Caddy auth endpoint validates it and sets a
-- session cookie on .spwn.town.

CREATE TABLE vm_auth_tokens (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  vm_id TEXT NOT NULL REFERENCES vms(id),
  expires_at BIGINT NOT NULL,
  created_at BIGINT NOT NULL
);

CREATE INDEX idx_vm_auth_tokens_expires ON vm_auth_tokens(expires_at);
