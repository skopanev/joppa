//! The check run of the manual checks (manual_checks.rs). At serve start and every 600 seconds
//! the server turns the expired passes of each workspace grey, without a journal event. The
//! stored status of a criterion that a pass with valid_until decides holds that date, so one
//! read finds the candidates. A run that changes no status writes nothing.
use super::*;
use std::collections::BTreeSet;

/// The time between two check runs.
const PERIOD: std::time::Duration = std::time::Duration::from_secs(600);

/// A stored green status from a pass that expires on or before the day $1. A date YYYY-MM-DD
/// orders as its text.
const DUE: &str = "test_status->>'status'='green' AND test_status->>'valid_until'<=$1::text";

/// Store the statuses that `day` gives to the criteria whose stored pass expires on or before
/// it, and the statuses that read them. Returns the number of criteria whose stored status
/// changed. The run waits for the turn of a writer and takes the workspace lock, as a write does.
pub async fn expire_manual_checks(
    pool: &Pool,
    workspace: &str,
    day: chrono::NaiveDate,
) -> Result<usize> {
    let _turn = super::writer::turn(workspace).await;
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    super::remote::lock(&*tx, &workspace).await?;
    let sql = format!("SELECT address FROM joppa.objects WHERE {DUE} AND workspace=$2");
    let candidates: Vec<String> = tx
        .query(&sql, &[&day.to_string(), &workspace])
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if candidates.is_empty() {
        return Ok(0);
    }
    let (state, position) = super::state_cache::take(&*tx, &workspace).await?;
    let work = store(&*tx, &workspace, &state, position, &candidates);
    let changed = crate::manual_checks::on_day(day, work).await?;
    let kept = super::state_cache::settle(&*tx, &workspace, state).await?;
    tx.commit().await?;
    if let Some(kept) = kept {
        kept.keep();
    }
    Ok(changed)
}

/// Store the new status of each candidate that changed, and the rollups above it.
async fn store(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    position: u64,
    candidates: &[String],
) -> Result<usize> {
    let statuses =
        super::reports::state_criterion_statuses(client, workspace, state, candidates).await?;
    let stored = super::objects::stored_statuses(client, workspace, candidates).await?;
    let changed: BTreeSet<String> = statuses
        .into_iter()
        .filter(|(address, status)| stored.get(address) != Some(status))
        .map(|(address, _)| address)
        .collect();
    let count = changed.len();
    if count == 0 {
        return Ok(0);
    }
    // The run adds no event: the projection stays at the position of the last write.
    let at = (position, position);
    let update =
        super::objects::update_status_projection(client, workspace, state, state, at, &[], changed);
    if !update.await? {
        super::objects::rebuild_status_projection(client, workspace, state, position).await?;
    }
    Ok(count)
}

/// The check run at serve start, then every 600 seconds. A failed or panicked run is logged,
/// and the loop goes on.
pub async fn manual_check_loop(pool: Pool) {
    loop {
        match tokio::spawn(run(pool.clone())).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("manual check run failed: {e:#}"),
            Err(e) => eprintln!("manual check run panicked: {e}"),
        }
        tokio::time::sleep(PERIOD).await;
    }
}

/// One run. One read finds the workspaces with a due pass, and only those take the turn of a
/// writer and the lock, with the UTC date of their own check. An idle run opens no transaction.
async fn run(pool: Pool) -> Result<()> {
    let today = || chrono::Utc::now().date_naive();
    let sql =
        format!("SELECT DISTINCT workspace FROM joppa.objects WHERE {DUE} ORDER BY workspace");
    let rows = pool
        .get()
        .await?
        .query(&sql, &[&today().to_string()])
        .await?;
    for row in rows {
        let workspace: String = row.get(0);
        if let Err(e) = expire_manual_checks(&pool, &workspace, today()).await {
            eprintln!("manual check run of {workspace} failed: {e:#}");
        }
    }
    Ok(())
}
