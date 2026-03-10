ALTER TABLE vm_events DROP CONSTRAINT vm_events_vm_id_fkey;

ALTER TABLE vm_events ADD CONSTRAINT vm_events_vm_id_fkey
  FOREIGN KEY (vm_id) REFERENCES vms(id) ON DELETE CASCADE;
