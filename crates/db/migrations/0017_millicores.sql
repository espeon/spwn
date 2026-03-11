-- vms.vcpus: double precision → bigint (millicores, 1000 = 1 vCPU)
ALTER TABLE vms ALTER COLUMN vcpus TYPE BIGINT USING ROUND(vcpus * 1000)::bigint;
ALTER TABLE vms ALTER COLUMN vcpus SET DEFAULT 1000;

-- accounts.vcpu_limit: integer → bigint (millicores)
ALTER TABLE accounts ALTER COLUMN vcpu_limit TYPE BIGINT USING (vcpu_limit * 1000)::bigint;
ALTER TABLE accounts ALTER COLUMN vcpu_limit SET DEFAULT 8000;

-- hosts.vcpu_total, vcpu_used: integer → bigint (millicores)
ALTER TABLE hosts ALTER COLUMN vcpu_total TYPE BIGINT USING (vcpu_total * 1000)::bigint;
ALTER TABLE hosts ALTER COLUMN vcpu_used TYPE BIGINT USING (vcpu_used * 1000)::bigint;
