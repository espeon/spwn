CREATE TABLE IF NOT EXISTS images (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    tag        TEXT NOT NULL DEFAULT 'latest',
    source     TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'building',
    size_bytes BIGINT NOT NULL DEFAULT 0,
    error      TEXT,
    created_at BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())::BIGINT,

    UNIQUE (name, tag)
);
