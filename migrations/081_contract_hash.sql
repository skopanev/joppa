-- The contract hash of each object from the objects projection (parent_hash.rs). A raise of
-- parent_hash_version fills it from the current rows at serve start.
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS contract_hash text;
