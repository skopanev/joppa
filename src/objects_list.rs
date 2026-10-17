//! The list read, part 1: the filters, the cursor, the waits and the work order.
use super::*;

/// What the list query needs from part 1.
pub(super) struct ListPlan<'s> {
    pub(super) related_to: Option<String>,
    pub(super) limit: usize,
    pub(super) inbox: bool,
    pub(super) after_processed: bool,
    pub(super) after: &'s str,
    pub(super) parent: Option<String>,
    pub(super) waits: std::collections::BTreeMap<String, String>,
    pub(super) counts: std::collections::BTreeMap<String, usize>,
    pub(super) waiting: Vec<String>,
    pub(super) after_filter: String,
    pub(super) order: String,
    pub(super) values: Vec<String>,
    pub(super) green: Vec<String>,
}

// The body moved from read_objects unchanged, so it keeps `&*conn` on a borrowed transaction.
#[allow(clippy::too_many_arguments, clippy::borrow_deref_ref)]
pub(super) async fn read_list(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: String,
    public: String,
    position: i64,
    prefix: String,
    selected: std::collections::BTreeMap<String, objects::TreeType>,
    task_kinds: Vec<String>,
    criterion_kinds: Vec<String>,
    specification_kinds: Vec<String>,
    unstarted: bool,
    scope: &ObjectScope<'_>,
) -> Result<Value> {
    ensure!(
        scope.revision.is_none() && !scope.history,
        "revision and history require an object address"
    );
    ensure!(
        scope
            .note_status
            .is_none_or(|status| ["unprocessed", "processed"].contains(&status)),
        "unknown Note status"
    );
    ensure!(
        scope.note_status.is_none()
            || scope
                .kind
                .is_some_and(|kind| selected.get(kind).is_some_and(|t| t.inbox.unwrap_or(false))),
        "note_status requires an inbox type"
    );
    let related_to = if let Some(key) = scope.related_to {
        let row = conn.query_opt("SELECT o.address FROM joppa.objects o WHERE o.workspace=$1 AND o.address=COALESCE((SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2),$2)", &[&workspace,&key]).await?.context("unknown related object")?;
        Some(row.get::<_, String>(0))
    } else {
        None
    };
    let limit = if scope.limit == 0 { 50 } else { scope.limit };
    ensure!((1..=100).contains(&limit), "limit must be 1 to 100");
    let inbox = scope
        .kind
        .is_some_and(|kind| selected.get(kind).is_some_and(|t| t.inbox.unwrap_or(false)));
    let (after_processed, after) = if let Some(cursor) = scope.cursor {
        let (p, key) = cursor.split_once(':').context("invalid object cursor")?;
        ensure!(
            p.parse::<i64>()? == position,
            "object list changed; start again without cursor"
        );
        if inbox {
            let (phase, address) = key
                .split_once(':')
                .context("invalid inbox cursor; start again without cursor")?;
            ensure!(
                ["0", "1"].contains(&phase)
                    && address
                        .split_once(':')
                        .is_some_and(|(kind, id)| Some(kind) == scope.kind
                            && !id.is_empty()
                            && !id.contains(':')),
                "invalid inbox cursor; start again without cursor"
            );
            (phase == "1", address)
        } else {
            ensure!(
                key.split_once(':')
                    .is_some_and(|(kind, id)| !kind.is_empty()
                        && !id.is_empty()
                        && !id.contains(':')),
                "invalid object cursor; start again without cursor"
            );
            (false, key)
        }
    } else {
        (false, "")
    };
    let parent = if let Some(parent) = scope.parent {
        Some(
            conn.query_opt(
                "SELECT target FROM joppa.object_aliases WHERE workspace=$1 AND address=$2",
                &[&workspace, &parent],
            )
            .await?
            .map(|row| row.get::<_, String>(0))
            .unwrap_or_else(|| parent.to_owned()),
        )
    } else {
        None
    };
    // Waiting criteria leave the unstarted list inside the query, before its limit.
    let (waits, counts) = super::waits::read_with_counts(&*conn, &workspace, &selected).await?;
    let waiting: Vec<String> = waits.keys().cloned().collect();
    // Unstarted work follows the priority chain from the top, then journal order. Its pages
    // continue after the cursor object in that order, at the same journal position.
    let chain = priority_chain_sql("o.workspace", "o.address", "$16::text[]");
    let unstarted_order = format!(
        "{chain},(SELECT (r.document->>'at')::timestamptz FROM joppa.object_revisions r WHERE r.workspace=o.workspace AND r.address=o.address AND r.revision=1),o.address"
    );
    let (after_filter, order) = if inbox {
        (
            "((o.inbox_status='processed'),o.address)>($13::boolean,$2::text) AND $16::text[] IS NOT NULL".to_owned(),
            "(o.inbox_status='processed'),o.address".to_owned(),
        )
    } else if unstarted {
        (
            "$2::text IS NOT NULL AND NOT $13::boolean".to_owned(),
            unstarted_order,
        )
    } else {
        (
            "o.address>$2 AND NOT $13::boolean AND $16::text[] IS NOT NULL".to_owned(),
            "o.address".to_owned(),
        )
    };
    let values = priority_values(&*conn, &workspace).await?;
    // A criterion that is green by its test reports has nothing left to plan.
    // It leaves the unstarted list before LIMIT, so pages stay full.
    let green: Vec<String> = if unstarted {
        let targets: Vec<String> = conn
            .query(
                "SELECT address FROM joppa.objects WHERE workspace=$1 AND present AND kind=ANY($2)",
                &[&workspace, &criterion_kinds],
            )
            .await?
            .iter()
            .map(|row| row.get(0))
            .collect();
        let optional = targets.iter().cloned().collect();
        super::reports::criterion_test_statuses(&*conn, &workspace, &targets, &optional)
            .await?
            .into_iter()
            .filter(|(_, status)| status["status"] == "green")
            .map(|(address, _)| address)
            .collect()
    } else {
        Vec::new()
    };
    let plan = ListPlan {
        related_to,
        limit,
        inbox,
        after_processed,
        after,
        parent,
        waits,
        counts,
        waiting,
        after_filter,
        order,
        values,
        green,
    };
    super::object_list_rows::list_rows(
        conn,
        workspace,
        public,
        position,
        prefix,
        selected,
        task_kinds,
        criterion_kinds,
        specification_kinds,
        unstarted,
        scope,
        plan,
    )
    .await
}
