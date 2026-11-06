-- The filter dimensions of the status projection (filter_dimensions.rs): the dimension values
-- that each node applies to, and its tone for each combination of dimension values.
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS dims jsonb;
ALTER TABLE joppa.objects ADD COLUMN IF NOT EXISTS dim_tones jsonb;
