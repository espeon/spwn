CREATE TABLE IF NOT EXISTS snapshots (
    id TEXT PRIMARY KEY,
    vm_id TEXT NOT NULL REFERENCES vms(id) ON DELETE CASCADE,
    label TEXT,
    snapshot_path TEXT NOT NULL,
    mem_path TEXT NOT NULL,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL
);
