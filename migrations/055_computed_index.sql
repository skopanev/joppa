-- The containment index of the computed values, as objects_data_contains is for the data.
CREATE INDEX IF NOT EXISTS objects_computed_contains ON joppa.objects USING gin (computed jsonb_path_ops);
