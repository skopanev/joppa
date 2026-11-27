//! The list read, part 3: the order of the request and the dependency facts of the page
//! (dependency_order.rs, dependency_sql.rs). A list with tiers gives the nodes of the first
//! tier first (node_filter_tiers.rs).
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
    ranks: Option<&crate::node_filter::Ranks>,
) -> Result<Sorted> {
    let order = scope.order.as_ref();
    let facts = match needs.dependency || order.is_some_and(|o| o.needs_facts()) {
        true => {
            let archived = scope.filter.archived();
            Some(crate::dependency_sql::facts(conn, workspace, archived).await?)
        }
        false => None,
    };
    if order.is_none() && ranks.is_none() {
        return Ok(Sorted {
            sql: None,
            params: vec![],
            facts,
        });
    }
    let mut params: Vec<Param> = vec![];
    let mut parts = vec![];
    if let Some(ranks) = ranks {
        params.push(Box::new(ranks.json.clone()));
        params.push(Box::new(ranks.rest));
        parts.push("COALESCE(($19::text::jsonb->>o.address)::int,$20) ASC".to_owned());
    }
    let Some(order) = order else {
        parts.push("o.address ASC".to_owned());
        return Ok(Sorted {
            sql: Some(parts.join(",")),
            params,
            facts,
        });
    };
    let priorities = match order.needs_priorities() {
        true => crate::dependency_order::priorities(conn, workspace).await?,
        false => vec![],
    };
    let narrowed: std::collections::BTreeMap<String, objects::TreeType>;
    let types = match &scope.filter.kinds {
        Some(kinds) => {
            narrowed = types
                .iter()
                .filter(|(k, _)| kinds.contains(k))
                .map(|(k, t)| (k.clone(), t.clone()))
                .collect();
            &narrowed
        }
        None => types,
    };
    let inputs = Inputs {
        facts: facts.as_ref().map(crate::dependency_sql::Facts::json),
        priorities: &priorities,
        types,
    };
    let (sql, more) = order.sql(19 + params.len(), inputs);
    parts.push(sql);
    params.extend(more);
    Ok(Sorted {
        sql: Some(parts.join(",")),
        params,
        facts,
    })
}
