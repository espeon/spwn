CREATE TABLE accounts (
  id TEXT PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  password_hash TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  subscription_status TEXT NOT NULL DEFAULT 'inactive',
  subscription_id TEXT,
  activated_at BIGINT
);

CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  expires_at BIGINT NOT NULL
);

CREATE TABLE vms (
  id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES accounts(id),
  name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'stopped',
  subdomain TEXT UNIQUE NOT NULL,
  vcores INTEGER NOT NULL DEFAULT 2,
  memory_mb INTEGER NOT NULL DEFAULT 512,
  disk_gb INTEGER NOT NULL DEFAULT 20,
  kernel_path TEXT NOT NULL,
  rootfs_path TEXT NOT NULL,
  snapshot_path TEXT,
  tap_device TEXT,
  ip_address TEXT NOT NULL,
  exposed_port INTEGER NOT NULL DEFAULT 8080,
  pid BIGINT,
  socket_path TEXT,
  created_at BIGINT NOT NULL,
  last_started_at BIGINT
);

CREATE TABLE vm_events (
  id BIGSERIAL PRIMARY KEY,
  vm_id TEXT NOT NULL REFERENCES vms(id),
  event TEXT NOT NULL,
  metadata TEXT,
  created_at BIGINT NOT NULL
);

CREATE TABLE processed_webhooks (
  event_id TEXT PRIMARY KEY,
  processed_at BIGINT NOT NULL
);

-- seed a dev account for phase 3 (no auth yet)
INSERT INTO accounts (id, email, password_hash, created_at, subscription_status)
VALUES ('dev', 'dev@localhost', 'none', 0, 'active')
ON CONFLICT DO NOTHING;
