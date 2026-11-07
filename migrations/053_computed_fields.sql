-- The values of the computed data fields of the status projection (computed_fields.rs): for
-- each node of a type that computes a field, each computed field with its value or null.
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS computed jsonb;
