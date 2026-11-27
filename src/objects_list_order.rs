//! The list read, part 3: the order of the request and the dependency facts of the page
//! (dependency_order.rs, dependency_sql.rs).
use super::*;
use crate::dependency_order::{Inputs, Param};

/// The order of a list and what it reads.
pub(super) struct Sorted {
    /// The ORDER BY text, or None for the default order of the list.
    pub(super) sql: Option<String>,
    /// The parameters of `sql`, from $19.
    pub(super) params: Vec<Param>,
    /// The facts, when the order or the fields read them.
    pub(super) facts: Option<crate::dependency_sql::Facts>,
}

/// The order of `scope` and the facts that it or `needs` reads, in the snapshot of `conn`.
pub(super) async fn sorted(
    conn: &tokio_postgres::Transaction<'_>,
    workspace: &str,
    scope: &ObjectScope<'_>,
    types: &std::collections::BTreeMap<String, objects::TreeType>,
    needs: CardNeeds,
) -> Result<Sorted> {
    let order = scope.order.as_ref();
    let facts = match needs.dependency || order.is_some_and(|o| o.needs_facts()) {
        true => {
            let archived = scope.filter.archived();
            Some(crate::dependency_sql::facts(conn, workspace, archived).await?)
        }
        false => None,
    };
    let Some(order) = order else {
        return Ok(Sorted {
            sql: None,
            params: vec![],
            facts,
        });
    };
    let priorities = match order.needs_priorities() {
        true => crate::dependency_order::priorities(conn, workspace).await?,
        false => vec![],
    };
    let inputs = Inputs {
        facts: facts.as_ref().map(crate::dependency_sql::Facts::json),
        priorities: &priorities,
        types,
    };
    let (sql, params) = order.sql(19, inputs);
    Ok(Sorted {
        sql: Some(sql),
        params,
        facts,
    })
}
