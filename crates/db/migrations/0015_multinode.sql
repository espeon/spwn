-- Host status, resource tracking, labels, and snapshot transfer address.
ALTER TABLE hosts
    ADD COLUMN IF NOT EXISTS status          TEXT    NOT NULL DEFAULT 'active',
    ADD COLUMN IF NOT EXISTS vcpu_used       INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS mem_used_mb     INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS labels          JSONB   NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS snapshot_addr   TEXT    NOT NULL DEFAULT '';

-- VM placement preferences and label constraints.
ALTER TABLE vms
    ADD COLUMN IF NOT EXISTS placement_strategy TEXT NOT NULL DEFAULT 'best_fit',
    ADD COLUMN IF NOT EXISTS required_labels     JSONB;

-- Migration history.
CREATE TABLE IF NOT EXISTS vm_migrations (
    id          TEXT    PRIMARY KEY,
    vm_id       TEXT    NOT NULL REFERENCES vms(id),
    from_host   TEXT    NOT NULL REFERENCES hosts(id),
    to_host     TEXT    NOT NULL REFERENCES hosts(id),
    status      TEXT    NOT NULL DEFAULT 'pending',
    started_at  BIGINT  NOT NULL,
    finished_at BIGINT
);

CREATE INDEX IF NOT EXISTS vm_migrations_vm_id ON vm_migrations(vm_id);
