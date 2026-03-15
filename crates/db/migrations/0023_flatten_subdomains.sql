-- Flatten VM subdomains from "{vm-name}.{username}" to "{vm-name}".
-- Uses ROW_NUMBER() to deduplicate collisions: first created VM keeps the
-- base slug, subsequent collisions get -2, -3, etc.
WITH ranked AS (
    SELECT
        v.id,
        split_part(v.subdomain, '.', 1) AS base_name,
        ROW_NUMBER() OVER (
            PARTITION BY split_part(v.subdomain, '.', 1)
            ORDER BY v.created_at, v.id
        ) AS rn
    FROM vms v
)
UPDATE vms
SET subdomain = CASE
    WHEN r.rn = 1 THEN r.base_name
    ELSE r.base_name || '-' || r.rn::text
END
FROM ranked r
WHERE vms.id = r.id;
