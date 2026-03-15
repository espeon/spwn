CREATE TABLE namespaces (
    id           TEXT PRIMARY KEY,
    slug         TEXT NOT NULL UNIQUE,
    kind         TEXT NOT NULL DEFAULT 'personal',
    display_name TEXT,
    owner_id     TEXT NOT NULL REFERENCES accounts(id),
    vcpu_limit   BIGINT NOT NULL DEFAULT 8000,
    mem_limit_mb INT NOT NULL DEFAULT 12288,
    vm_limit     INT NOT NULL DEFAULT 5,
    created_at   BIGINT NOT NULL
);

CREATE INDEX namespaces_owner_id ON namespaces(owner_id);

CREATE TABLE namespace_members (
    namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role         TEXT NOT NULL DEFAULT 'member',
    joined_at    BIGINT NOT NULL,
    PRIMARY KEY (namespace_id, account_id)
);

CREATE INDEX namespace_members_account ON namespace_members(account_id);

-- Backfill: one personal namespace per existing account (slug = username)
INSERT INTO namespaces (id, slug, kind, display_name, owner_id, vcpu_limit, mem_limit_mb, vm_limit, created_at)
SELECT
    'ns_' || id,
    username,
    'personal',
    username,
    id,
    vcpu_limit,
    mem_limit_mb,
    vm_limit,
    created_at
FROM accounts;

INSERT INTO namespace_members (namespace_id, account_id, role, joined_at)
SELECT 'ns_' || id, id, 'owner', created_at
FROM accounts;

-- Add namespace_id to VMs
ALTER TABLE vms ADD COLUMN namespace_id TEXT REFERENCES namespaces(id);

UPDATE vms SET namespace_id = 'ns_' || account_id;

ALTER TABLE vms ALTER COLUMN namespace_id SET NOT NULL;
