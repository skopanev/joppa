-- The `data` filter of joppa_objects asks whether document.data contains a JSON object.
-- jsonb_path_ops serves the @> operator with a small index.
CREATE INDEX IF NOT EXISTS objects_data_contains ON joppa.objects USING gin ((document->'data') jsonb_path_ops);
