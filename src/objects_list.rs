//! The list read, part 1: the filters, the cursor and the order.
use super::*;

/// What the list query needs from part 1.
pub(super) struct ListPlan<'s> {
    pub(super) related_to: Option<String>,
    pub(super) limit: usize,
    pub(super) inbox: bool,
    pub(super) after_processed: bool,
    pub(super) after: &'s str,
    pub(super) bound: crate::node_filter::Bound,
    pub(super) after_filter: String,
    pub(super) order: String,
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
                .filter
                .one_kind()
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
        .filter
        .one_kind()
        .is_some_and(|kind| selected.get(kind).is_some_and(|t| t.inbox.unwrap_or(false)));
    let (after_processed, after) = if let Some(cursor) = scope.cursor {
        let (p, key) = cursor.split_once(':').context("invalid object cursor")?;
        // A list in address order also takes the bare address of its last item, as the
        // compact MCP list gives it. That cursor stays valid across writes.
        if p.parse::<i64>().is_err() && !inbox {
            ensure!(
                address_cursor(cursor),
                "invalid object cursor; start again without cursor"
            );
            (false, cursor)
        } else {
            ensure!(
                p.parse::<i64>().ok() == Some(position),
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
                            .is_some_and(|(kind, id)| Some(kind) == scope.filter.one_kind()
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
        }
    } else {
        (false, "")
    };
    // Without a parent and without a filter, a list gives the roots. A related_to list reads
    // every level.
    // A status list with the archived tone also keeps the current archived nodes.
    let bound = crate::node_filter::bind(
        conn,
        &workspace,
        &scope.filter,
        related_to.is_none(),
        scope.filter.archived(),
    )
    .await?;
    let (after_filter, order) = if inbox {
        (
            "((o.inbox_status='processed'),o.address)>($9::boolean,$2::text)".to_owned(),
            "(o.inbox_status='processed'),o.address".to_owned(),
        )
    } else {
        (
            "o.address>$2 AND NOT $9::boolean".to_owned(),
            "o.address".to_owned(),
        )
    };
    let plan = ListPlan {
        related_to,
        limit,
        inbox,
        after_processed,
        after,
        bound,
        after_filter,
        order,
    };
    super::object_list_rows::list_rows(
        conn, workspace, public, position, prefix, selected, scope, plan,
    )
    .await
}

/// An object address `kind:id` with no further colon.
pub(super) fn address_cursor(cursor: &str) -> bool {
    cursor
        .split_once(':')
        .is_some_and(|(kind, id)| !kind.is_empty() && !id.is_empty() && !id.contains(':'))
}
