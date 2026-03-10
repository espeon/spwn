ALTER TABLE accounts
  ADD COLUMN IF NOT EXISTS username     TEXT,
  ADD COLUMN IF NOT EXISTS display_name TEXT,
  ADD COLUMN IF NOT EXISTS avatar_bytes BYTEA;

UPDATE accounts SET username = id WHERE username IS NULL;

ALTER TABLE accounts ALTER COLUMN username SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS accounts_username_idx ON accounts (username);
