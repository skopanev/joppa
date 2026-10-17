//! The scoped read of one workspace.
use super::*;

/// Что именно просят прочитать.
///
/// Появилось не от любви к настройкам: `read` отдавал воркспейс целиком —
/// состояние со всеми ревизиями, AC, задачами, прогонами и находками, плюс
/// ВСЮ историю событий. На живом воркспейсе это 165 тысяч знаков: столько не
/// влезает ни в один контекст, и человек шёл выгружать дамп и разбирать его
/// скриптом вместо того, чтобы спросить.
///
/// Два слагаемых растут без предела — история и количество требований, — и
/// одно ограничение их не покрывает. Поэтому здесь не «страница», а
/// постепенное раскрытие: сначала оглавление, потом то, что попросили.
pub struct Scope {
    /// Full state for the browser and administrative snapshots only.
    pub full: bool,
    /// One requirement, current revision unless a number is requested.
    pub req: Option<String>,
    pub revision: Option<u32>,
    /// Revision counts only, never an event or revision dump.
    pub history: bool,
    pub target: Option<String>,
}
impl Scope {
    pub fn full() -> Self {
        Self {
            full: true,
            req: None,
            revision: None,
            history: false,
            target: None,
        }
    }
    pub fn from_args(a: &Value) -> Result<Self> {
        ensure!(
            a.get("cursor").is_none() && a.get("limit").is_none(),
            "history returns revision counts; request one revision by number, not cursor or limit"
        );
        let revision = a
            .get("revision")
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .context("revision must be a positive integer")
            })
            .transpose()?;
        let history = a
            .get("history")
            .map(|value| value.as_bool().context("history must be a boolean"))
            .transpose()?
            .unwrap_or(false);
        let req = a
            .get("req")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .context("req must be a string")
            })
            .transpose()?;
        let target = a
            .get("target")
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .context("target must be a string")
            })
            .transpose()?;
        ensure!(req.is_none() || target.is_none(), "supply req OR target");
        ensure!(
            target.is_none() || (revision.is_none() && !history),
            "revision and history require a requirement, not a notes target"
        );
        Ok(Self {
            full: false,
            req,
            revision,
            history,
            target,
        })
    }
}

pub async fn read(pool: &Pool, workspace: &str, actor: &str, scope: &Scope) -> Result<Value> {
    let mut client = pool.get().await?;
    let conn = client
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let resolved = resolve_workspace(&*conn, workspace).await?;
    let workspace = resolved.as_str();
    access::check(&*conn, workspace, actor, false).await?;
    if !scope.full && scope.req.is_none() && scope.target.is_none() {
        ensure!(
            scope.revision.is_none(),
            "revision requires an object or verification context"
        );
        let row = conn
            .query_one(
                "SELECT first_view::text,(SELECT value->>'admin' FROM joppa.state_entries WHERE workspace=$1 AND section='meta' AND key='') FROM joppa.board_cache WHERE workspace=$1",
                &[&workspace],
            )
            .await?;
        let first: Value = serde_json::from_str(row.get(0))?;
        let owner: Option<String> = row.get(1);
        drop(conn);
        drop(client);
        let roots = read_objects(pool, workspace, actor, &ObjectScope::default()).await?;
        return Ok(
            json!({"workspace":roots["workspace"],"position":roots["position"],"index":{"members":first["members"],"owner":owner,"types":first["object_types"],"objects":roots["items"],"next_cursor":roots["next_cursor"]},"launch":first["launch"],"reading":"Read an object and its children with joppa_objects. Read configured verification evidence with req or a copied item_id. Request revision=N for one older revision."}),
        );
    }
    if !scope.full {
        if let Some(req) = &scope.req {
            return state_rows::read_requirement(&*conn, workspace, req, scope).await;
        }
        if let Some(target) = &scope.target {
            ensure!(
                scope.revision.is_none(),
                "revision requires req or a requirement/AC item_id"
            );
            drop(conn);
            drop(client);
            let object = read_objects(
                pool,
                workspace,
                actor,
                &ObjectScope {
                    address: Some(target),
                    ..Default::default()
                },
            )
            .await?;
            return Ok(
                json!({"workspace":object["workspace"],"position":object["position"],"target":target,"notes":object["annotations"]}),
            );
        }
    }
    let (state, events) = state_rows::full(&*conn, workspace).await?;
    ensure!(state.members.contains_key(actor), access::AccessDenied);
    let mut system_ids = json!({});
    for row in conn
        .query(
            "SELECT kind,local_id,id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2",
            &[
                &workspace,
                &state.reference_prefix.as_deref().unwrap_or(workspace),
            ],
        )
        .await?
    {
        let kind: String = row.get(0);
        let local: String = row.get(1);
        let id: String = row.get(2);
        system_ids[&kind][&local] = json!(id);
    }
    let summaries = summaries(&state);
    let launch = crate::objects::launch_summary(&state);
    let mut out = json!({
        "workspace": state.workspace_id.as_deref().unwrap_or(workspace),
        "system_ids": system_ids,
        "consumers": state.consumers,
        "position": events.len(),
        "launch": launch,
    });

    if scope.full {
        let humans: Vec<_> = state
            .members
            .iter()
            .filter(|(_, role)| role.as_str() == "human")
            .map(|(id, _)| id.clone())
            .collect();
        let mut labels = serde_json::Map::new();
        for row in conn.query(
            "SELECT actor, COALESCE(NULLIF(btrim(display_name),''),split_part(email,'@',1)) FROM joppa.google_identities WHERE actor = ANY($1)",
            &[&humans],
        ).await? {
            let actor: String = row.get(0);
            let label: String = row.get(1);
            if !label.is_empty() {
                labels.insert(actor, json!(label));
            }
        }
        out["member_labels"] = json!(labels);
        out["state"] = json!(state);
        out["summary"] = json!(summaries);
        out["history"] = json!(events);
        return Ok(out);
    }

    Ok(out)
}
