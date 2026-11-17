//! The reports of each workspace in memory. A record only adds reports and receipts, so the
//! number of reports, the newest report and the newest receipt name one list of reports. A
//! status read compares this key and reads the list again only when the key changes.
use super::*;
use crate::ac_tests::Report;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// The number of reports, the newest report and the newest receipt.
type Key = (i64, i64, Option<i64>);

/// The kept list of each workspace with its key.
type Kept = HashMap<String, (Key, Arc<Vec<Report>>)>;

static REPORTS: LazyLock<Mutex<Kept>> = LazyLock::new(Default::default);

/// The reports of `workspace` on the default branch that are not dirty.
pub(super) async fn reports(
    client: &impl GenericClient,
    workspace: &str,
) -> Result<Arc<Vec<Report>>> {
    let sql = "SELECT count(*),COALESCE(max(r.id),0),(SELECT (extract(epoch FROM max(c.received_at))*1000000)::bigint FROM joppa.test_report_receipts c JOIN joppa.test_reports x ON x.id=c.report_id WHERE x.workspace=$1) FROM joppa.test_reports r WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty";
    let row = client.query_one(sql, &[&workspace]).await?;
    let key: Key = (row.get(0), row.get(1), row.get(2));
    if let Some((kept, list)) = REPORTS.lock().unwrap().get(workspace)
        && *kept == key
    {
        return Ok(list.clone());
    }
    // The receipts of the workspace aggregate once: a lookup for each report would scan the
    // small receipts table once for each report.
    let sql = "SELECT r.id,r.commit_sha,r.commit_order,(extract(epoch FROM r.received_at)*1000000)::bigint,(extract(epoch FROM greatest(r.received_at,c.last))*1000000)::bigint,r.recorded FROM joppa.test_reports r LEFT JOIN (SELECT c.report_id,max(c.received_at) AS last FROM joppa.test_report_receipts c JOIN joppa.test_reports x ON x.id=c.report_id WHERE x.workspace=$1 GROUP BY c.report_id) c ON c.report_id=r.id WHERE r.workspace=$1 AND r.default_branch AND NOT r.dirty";
    let list: Vec<Report> = client
        .query(sql, &[&workspace])
        .await?
        .iter()
        .map(|r| Report {
            id: r.get(0),
            commit: r.get(1),
            order: r.get(2),
            received_us: r.get(3),
            receipt_us: r.get(4),
            recorded: r.get(5),
        })
        .collect();
    let list = Arc::new(list);
    let mut kept = REPORTS.lock().unwrap();
    kept.insert(workspace.to_owned(), (key, list.clone()));
    Ok(list)
}
