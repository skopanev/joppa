-- A write that records results reads the test nodes of its report by report_id
-- (objects_projection_delta.rs). Without this index the read scans every result row.
CREATE INDEX IF NOT EXISTS test_results_by_report ON joppa.test_results(report_id, address);
