//! Row decoration: the waits.
use super::*;

/// Waiting work reads work_ready=false and names its first unsatisfied target with its
/// title.
pub(super) async fn add_waits(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
    waits: &std::collections::BTreeMap<String, String>,
    counts: &std::collections::BTreeMap<String, usize>,
) -> Result<()> {
    crate::probe::count(&crate::probe::ROLLUP);
    let targets: Vec<&String> = items
        .iter()
        .filter_map(|item| waits.get(item["address"].as_str()?))
        .collect();
    if targets.is_empty() {
        return Ok(());
    }
    let titles: std::collections::BTreeMap<String, String> = client
        .query(
            "SELECT address,title FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)",
            &[&workspace, &targets],
        )
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    for item in items {
        let Some(target) = item["address"].as_str().and_then(|a| waits.get(a)) else {
            continue;
        };
        let reason = crate::dependencies::reason(target);
        item["work_ready"] = json!(false);
        // The row shows only the count; the first target stays for the reason and the refusal.
        let count = counts
            .get(item["address"].as_str().unwrap_or(""))
            .copied()
            .unwrap_or(1);
        item["waits_for"] =
            json!({"address":target,"title":titles.get(target),"reason":reason,"count":count});
    }
    Ok(())
}
