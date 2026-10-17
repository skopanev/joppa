//! JUnit report upload: parse, store and record one clean or dirty test run.
use super::*;

pub async fn upload_report(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    upload: ReportUpload,
) -> Result<Value> {
    let stamp = &upload.stamp;
    ensure!(upload.junit.len() <= REPORT_MAX, "a report is at most 5 MB");
    ensure!(
        (7..=64).contains(&stamp.commit.len())
            && stamp
                .commit
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "stamp.commit must be a lowercase git sha"
    );
    ensure!(!stamp.branch.trim().is_empty(), "stamp.branch is required");
    let mut results = parse_junit(&upload.junit)?;
    for result in &mut results {
        if let Some(message) = result.message.as_mut().filter(|m| m.len() > MESSAGE_MAX) {
            let mut end = MESSAGE_MAX;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
    }
    let stamp_hash = format!(
        "{:x}",
        Sha256::digest(format!("{}\n{}", serde_json::to_string(stamp)?, upload.junit).as_bytes())
    );
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    let workspace = resolve_workspace(&*tx, workspace).await?;
    access::check(&*tx, &workspace, actor, false).await?;
    if stamp.dirty {
        return Err(commands::ClientField("a dirty run cannot be uploaded").into());
    }
    // Writes take the workspace lock first; the upload appends its journal event under it.
    tx.execute(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
        &[&workspace],
    )
    .await?;
    let event = |report: i64, repeat: bool| Command::RecordReport {
        report,
        commit: stamp.commit.clone(),
        branch: stamp.branch.clone(),
        tests: results.len() as u64,
        repeat,
    };
    let inserted = tx.query_opt(
        "INSERT INTO joppa.test_reports(workspace,commit_sha,commit_time,commit_order,branch,default_branch,dirty,full_run,uploader,stamp_hash,junit) VALUES($1,$2,$3::text::timestamptz,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT (workspace,commit_sha,stamp_hash) DO NOTHING RETURNING id",
        &[&workspace,&stamp.commit,&stamp.commit_time,&stamp.order,&stamp.branch,&stamp.default_branch,&stamp.dirty,&stamp.full,&actor,&stamp_hash,&upload.junit],
    ).await?;
    let Some(row) = inserted else {
        let id: i64 = tx.query_one("SELECT id FROM joppa.test_reports WHERE workspace=$1 AND commit_sha=$2 AND stamp_hash=$3", &[&workspace,&stamp.commit,&stamp_hash]).await?.get(0);
        // A clean repeat is a new receipt of the same report: it counts as received now.
        tx.execute(
            "INSERT INTO joppa.test_report_receipts(report_id) VALUES($1)",
            &[&id],
        )
        .await?;
        let position =
            super::timeline::record_report(&*tx, &workspace, actor, event(id, true)).await?;
        tx.commit().await?;
        return Ok(
            json!({"report": id, "tests": results.len(), "replayed": true, "position": position}),
        );
    };
    let id: i64 = row.get(0);
    tx.execute(
        "INSERT INTO joppa.test_results(report_id,address,status,duration_ms,message) SELECT $1,x.address,x.status,x.duration_ms,x.message FROM jsonb_to_recordset($2::text::jsonb) x(address text,status text,duration_ms bigint,message text)",
        &[&id, &serde_json::to_string(&results)?],
    ).await?;
    let position =
        super::timeline::record_report(&*tx, &workspace, actor, event(id, false)).await?;
    tx.commit().await?;
    Ok(
        json!({"report": id, "commit": stamp.commit, "tests": results.len(), "replayed": false, "uploader": actor, "position": position}),
    )
}
