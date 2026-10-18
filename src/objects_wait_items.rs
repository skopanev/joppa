//! Row decoration: the waits and the priorities.
use super::*;

/// Waiting work reads work_ready=false and names its first unsatisfied target with its
/// title. A waiting criterion without an assessment blocker reads the dependency as its
/// readiness blocker.
pub(super) async fn add_waits(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
    waits: &std::collections::BTreeMap<String, String>,
    counts: &std::collections::BTreeMap<String, usize>,
    criteria: &[String],
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
        if criteria.iter().any(|kind| item["kind"] == kind.as_str())
            && item["process"].is_object()
            && item["process"]["readiness_blocker"].is_null()
        {
            item["process"]["readiness_blocker"] =
                json!({"state":"dependency","target":target,"reason":reason});
        }
    }
    Ok(())
}

/// The priority of each node in process.priority: its own value of the current list, else
/// the last value. A priority is not a change of the node.
pub(super) async fn add_priorities(
    client: &impl GenericClient,
    workspace: &str,
    items: &mut [Value],
) -> Result<()> {
    let addresses: Vec<String> = items
        .iter()
        .filter_map(|i| i["address"].as_str().map(String::from))
        .collect();
    if addresses.is_empty() {
        return Ok(());
    }
    let values = priority_values(client, workspace).await?;
    let own: std::collections::BTreeMap<String, String> = client
        .query(
            "SELECT key,value#>>'{}' FROM joppa.state_entries WHERE workspace=$1 AND section='priorities' AND key=ANY($2)",
            &[&workspace, &addresses],
        )
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    let last = values.last().cloned().unwrap_or_default();
    for item in items {
        let Some(address) = item["address"].as_str() else {
            continue;
        };
        let priority = own
            .get(address)
            .filter(|v| values.contains(v))
            .cloned()
            .unwrap_or_else(|| last.clone());
        if !item["process"].is_object() {
            item["process"] = json!({});
        }
        item["process"]["priority"] = json!(priority);
    }
    Ok(())
}
