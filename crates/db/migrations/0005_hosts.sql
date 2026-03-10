CREATE TABLE IF NOT EXISTS hosts (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    address      TEXT NOT NULL,
    vcpu_total   INTEGER NOT NULL DEFAULT 0,
    mem_total_mb INTEGER NOT NULL DEFAULT 0,
    images_dir   TEXT NOT NULL DEFAULT '/var/lib/spwn/images',
    overlay_dir  TEXT NOT NULL DEFAULT '/var/lib/spwn/overlays',
    snapshot_dir TEXT NOT NULL DEFAULT '/var/lib/spwn/snapshots',
    kernel_path  TEXT NOT NULL DEFAULT '/tmp/vmlinux',
    last_seen_at BIGINT NOT NULL DEFAULT 0
);

ALTER TABLE vms ADD COLUMN IF NOT EXISTS host_id TEXT REFERENCES hosts(id);
