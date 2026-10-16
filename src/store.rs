use crate::{
    checks,
    commands::{self, Command},
    model::Workspace,
};
use anyhow::{Context, Result, ensure};
use deadpool_postgres::Pool;
use serde::Serialize;
use serde_json::{Value, json};
use tokio_postgres::GenericClient;
#[path = "store_access.rs"]
mod access;
pub use access::{
    AccessDenied, enroll, enroll_granted, grant_domain, grant_email, granted_workspaces,
    list_domain_grants, list_email_grants, update_identity_name,
};
#[path = "store_board.rs"]
mod board;
pub use board::board_read;
#[path = "store_objects.rs"]
mod objects;
pub use objects::{ObjectScope, read_objects, tree_schema};
#[path = "store_grants.rs"]
mod grants;
#[path = "store_queue.rs"]
mod queue_rows;
pub use grants::{
    Access, READ_ONLY_REFUSAL, TokenGrant, create_agent_token, list_agent_tokens,
    revoke_agent_token, token_actor,
};
#[path = "store_reports.rs"]
mod reports;
pub use reports::{
    REPORT_MAX, ReportUpload, Stamp, criterion_tests, task_status, tests_overview, upload_report,
};
#[path = "store_tasks.rs"]
mod tasks;
pub use tasks::{task_queue, task_queue_filtered};
#[path = "store_waits.rs"]
mod waits;

pub const DEFAULT_URL: &str = "postgresql://postgres:joppa@127.0.0.1:55433/joppa";
pub fn pool(url: &str) -> Result<Pool> {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(url.to_string());
    Ok(cfg.create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?)
}
/// Migrations are repeatable, and Postgres aborts one side of a deadlock with a running
/// transaction (SQLSTATE 40P01). A deadlocked migration simply runs again.
pub async fn migrate(pool: &Pool) -> Result<()> {
    for attempt in 1.. {
        match migrate_once(pool).await {
            Err(e) if attempt < 3 && is_deadlock(&e) => {
                // Visible on purpose: a retried migration must show up in the logs.
                eprintln!("joppa: migration attempt {attempt} hit a deadlock, retrying: {e:#}");
                tokio::time::sleep(std::time::Duration::from_millis(100 * attempt)).await;
            }
            done => return done,
        }
    }
    unreachable!()
}
fn is_deadlock(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause
            .downcast_ref::<tokio_postgres::Error>()
            .and_then(tokio_postgres::Error::code)
            .is_some_and(|code| *code == tokio_postgres::error::SqlState::T_R_DEADLOCK_DETECTED)
    })
}
async fn migrate_once(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    tx.batch_execute(include_str!("../migrations/001_journal.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/002_item_ids.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/003_reference_prefix.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/004_cli_credentials.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/005_mcp_oauth.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/006_workspace_aliases.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/007_command_receipts.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/008_search.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/009_annotations.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/010_consumers.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/011_board_cache.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/012_objects.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/013_object_aliases.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/014_identity_names.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/015_current_state.sql"))
        .await?;
    tx.batch_execute(include_str!(
        "../migrations/016_current_item_references.sql"
    ))
    .await?;
    tx.batch_execute(include_str!("../migrations/017_test_reports.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/018_agent_tokens.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/019_inbox_order.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/020_task_claims.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/021_agent_token_access.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/022_email_grants.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/023_test_report_receipts.sql"))
        .await?;
    tx.batch_execute(include_str!("../migrations/024_domain_grants.sql"))
        .await?;
    for row in tx.query("SELECT w.id FROM joppa.workspaces w LEFT JOIN joppa.board_cache b ON b.workspace=w.id WHERE b.workspace IS NULL OR b.first_view='{}'::jsonb OR NOT EXISTS(SELECT 1 FROM joppa.state_entries s WHERE s.workspace=w.id AND s.section='meta') OR NOT (b.first_view ? 'object_types') OR COALESCE(b.first_view->>'projection_version','')<>$1", &[&BOARD_PROJECTION_VERSION]).await? {
        let id: String = row.get(0);
        tx.execute("SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))", &[&id]).await?;
        let (state, events) = load(&*tx, &id).await?;
        board::project(&*tx, &id, &state, &events).await?;
        search::project(&*tx, &id, &state, events.last().map_or(0, |e| e.position), None).await?;
    }
    migrate_notes_in(&*tx).await?;
    refresh_readiness_in(&*tx).await?;
    write::migrate_legacy_receipts(&*tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn migrate_notes(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    migrate_notes_in(&*tx).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn check_journal(pool: &Pool) -> Result<()> {
    let conn = pool.get().await?;
    for row in conn
        .query("SELECT id FROM joppa.workspaces ORDER BY id", &[])
        .await?
    {
        let _: (Workspace, Vec<Event>) = load(&**conn, row.get(0)).await?;
    }
    Ok(())
}
async fn migrate_notes_in(client: &impl GenericClient) -> Result<()> {
    for row in client
        .query("SELECT w.id FROM joppa.workspaces w JOIN joppa.board_cache b ON b.workspace=w.id WHERE NOT COALESCE((b.state->>'notes_migrated')::boolean,false) OR NOT COALESCE((b.state->>'tasks_migrated')::boolean,false) OR NOT COALESCE((b.state->>'schema_migrated')::boolean,false) OR NOT COALESCE((b.state->>'documents_migrated')::boolean,false) OR NOT COALESCE((b.state->>'claims_migrated')::boolean,false) OR NOT COALESCE((b.state->>'persistent_claims')::boolean,false) ORDER BY w.id", &[])
        .await?
    {
        let workspace: String = row.get(0);
        client
            .execute(
                "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
                &[&workspace],
            )
            .await?;
        let (mut state, old_events) = state_rows::full(client, &workspace).await?;
        let position = old_events.last().map_or(0, |e|e.position);
        let previous = state.clone();
        let mut events = Vec::new();
        if ensure_notes(client, &workspace, &mut state, &mut events, position).await? {
            search::project(
                client,
                &workspace,
                &state,
                position + events.len() as u64,
                None,
            )
            .await?;
            board::project_current(
                client,
                &workspace,
                &state,
                &events,
                position + events.len() as u64,
                Some(&previous),
            )
            .await?;
        }
    }
    Ok(())
}
async fn ensure_notes(
    client: &impl GenericClient,
    workspace: &str,
    state: &mut Workspace,
    events: &mut Vec<Event>,
    base_position: u64,
) -> Result<bool> {
    let mut changed = false;
    for (needed, command) in [
        (!state.notes_migrated, Command::MigrateNotes {}),
        (!state.tasks_migrated, Command::MigrateTasks {}),
        (!state.schema_migrated, Command::MigrateSchema {}),
        (!state.documents_migrated, Command::MigrateDocuments {}),
        (!state.claims_migrated, Command::MigrateTaskClaims {}),
        (
            !state.persistent_claims,
            Command::MigratePersistentClaims {},
        ),
    ] {
        if !needed {
            continue;
        }
        ensure!(!state.admin.is_empty(), "workspace has no owner");
        let position = events
            .last()
            .map_or(base_position + 1, |event| event.position + 1);
        let actor = state.admin.clone();
        let payload = serde_json::to_string(&command)?;
        let recorded_at: String = client.query_one("INSERT INTO joppa.events(workspace,position,actor,request_id,command) VALUES($1,$2,$3,$4,$5::text::jsonb) RETURNING recorded_at::text", &[&workspace,&(position as i64),&actor,&uuid::Uuid::new_v4().to_string(),&payload]).await?.get(0);
        commands::apply_replay(state, &actor, &command, position, &recorded_at)?;
        let actor_type = crate::inbox::actor_type(state, &actor).to_owned();
        events.push(Event {
            position,
            actor,
            actor_type,
            recorded_at,
            command,
        });
        changed = true;
    }
    Ok(changed)
}
pub(super) const BOARD_PROJECTION_VERSION: &str = "8";
pub(super) const READINESS_METADATA_VERSION: &str = "2";

/// Refresh current assessment and verification metadata without reading journal history.
pub async fn refresh_readiness_metadata(pool: &Pool) -> Result<()> {
    let mut conn = pool.get().await?;
    let tx = conn.transaction().await?;
    tx.execute("SELECT pg_advisory_xact_lock(91827101)", &[])
        .await?;
    refresh_readiness_in(&*tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn refresh_readiness_in(client: &impl GenericClient) -> Result<()> {
    for row in client.query("SELECT workspace FROM joppa.board_cache WHERE COALESCE(first_view->>'readiness_metadata_version','')<>$1 ORDER BY workspace", &[&READINESS_METADATA_VERSION]).await? {
        let workspace: String = row.get(0);
        client.execute("SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))", &[&workspace]).await?;
        let (state, _) = state_rows::current(client, &workspace).await?;
        let schema = crate::objects::schema(&state);
        let mut selected: std::collections::BTreeSet<_> = state.objects.iter().filter(|(_, o)| {
            schema.types.get(&o.kind).is_some_and(|t| matches!(t.role.as_str(), "ac" | "task"))
        }).map(|(key, _)| key.clone()).collect();
        loop {
            let dependents: Vec<_> = state.objects.iter().filter(|(key, o)| {
                !selected.contains(*key) && (o.current().parent.as_deref().is_some_and(|parent| selected.contains(crate::objects::resolve_address(&state,parent)))
                    || o.current().links.iter().any(|link| link.relation == "depends_on" && selected.contains(crate::objects::resolve_address(&state,&link.target)))
                    || o.current().data["depends_on"].as_array().is_some_and(|deps| deps.iter().filter_map(Value::as_str).any(|key| selected.contains(&crate::objects::criterion_address(key)))))
            }).map(|(key,_)|key.clone()).collect();
            if dependents.is_empty() { break; }
            selected.extend(dependents);
        }
        objects::project(client, &workspace, &state, &[], Some(&selected)).await?;
        queue_rows::project(client, &workspace, &state, Some(&selected)).await?;
        client.execute("UPDATE joppa.board_cache SET first_view=jsonb_set(first_view,'{readiness_metadata_version}',to_jsonb($2::text)) WHERE workspace=$1", &[&workspace,&READINESS_METADATA_VERSION]).await?;
    }
    Ok(())
}

pub async fn backfill_board(pool: &Pool) -> Result<usize> {
    let mut conn = pool.get().await?;
    let rows = conn.query("SELECT w.id FROM joppa.workspaces w LEFT JOIN joppa.board_cache b ON b.workspace=w.id WHERE b.workspace IS NULL OR b.first_view='{}'::jsonb OR NOT EXISTS(SELECT 1 FROM joppa.state_entries s WHERE s.workspace=w.id AND s.section='meta') OR NOT (b.first_view ? 'object_types') OR COALESCE(b.first_view->>'projection_version','')<>$1 OR b.position<>(SELECT COALESCE(MAX(e.position),0) FROM joppa.events e WHERE e.workspace=w.id) OR EXISTS(SELECT 1 FROM joppa.task_queue q WHERE q.workspace=w.id AND q.created_at=0) ORDER BY w.id", &[&BOARD_PROJECTION_VERSION]).await?;
    let mut done = 0;
    for row in rows {
        let id: String = row.get(0);
        let tx = conn.transaction().await?;
        tx.execute(
            "SELECT pg_advisory_xact_lock(hashtextextended($1,91827102))",
            &[&id],
        )
        .await?;
        let (state, events) = load(&*tx, &id).await?;
        board::project(&*tx, &id, &state, &events).await?;
        write::migrate_legacy_receipts_workspace(&*tx, &id).await?;
        search::project(
            &*tx,
            &id,
            &state,
            events.last().map_or(0, |e| e.position),
            None,
        )
        .await?;
        tx.commit().await?;
        done += 1;
    }
    Ok(done)
}
#[derive(Serialize)]
pub struct Event {
    pub position: u64,
    pub actor: String,
    pub actor_type: String,
    pub recorded_at: String,
    pub command: Command,
}
async fn load(client: &impl GenericClient, workspace: &str) -> Result<(Workspace, Vec<Event>)> {
    let rows = client.query("SELECT position,actor,recorded_at::text,command::text FROM joppa.events WHERE workspace=$1 ORDER BY position", &[&workspace]).await.context("run joppa migrate before using the journal")?;
    let mut state = Workspace::default();
    let mut events = vec![];
    for row in rows {
        let position = row.get::<_, i64>(0) as u64;
        let actor: String = row.get(1);
        let command: Command = serde_json::from_str(row.get::<_, &str>(3))?;
        commands::apply_replay(&mut state, &actor, &command, position, row.get(2))
            .with_context(|| format!("invalid journal entry at {workspace}/{position}"))?;
        let actor_type = crate::inbox::actor_type(&state, &actor).to_owned();
        events.push(Event {
            position,
            actor,
            actor_type,
            recorded_at: row.get(2),
            command,
        });
    }
    Ok((state, events))
}
async fn resolve_workspace(client: &impl GenericClient, name: &str) -> Result<String> {
    Ok(client
        .query_opt(
            "SELECT workspace FROM joppa.workspace_aliases WHERE name=$1",
            &[&name],
        )
        .await?
        .map(|r| r.get(0))
        .unwrap_or_else(|| name.to_owned()))
}
async fn reserve_name(client: &impl GenericClient, name: &str, workspace: &str) -> Result<()> {
    client.execute("INSERT INTO joppa.workspace_aliases(name,workspace) VALUES($1,$2) ON CONFLICT DO NOTHING", &[&name,&workspace]).await?;
    ensure!(
        resolve_workspace(client, name).await? == workspace,
        "workspace ID is already in use"
    );
    Ok(())
}
#[path = "store_state.rs"]
mod state_rows;
#[path = "store_write.rs"]
mod write;
pub use write::{
    BatchOperation, check_fields, execute, execute_batch, execute_input, execute_remote_batch,
    execute_remote_input, save_dependency,
};
fn summaries(state: &Workspace) -> Value {
    let summaries: serde_json::Map<String, Value> = state
        .requirements
        .iter()
        .map(|(id, q)| {
            (
                id.clone(),
                crate::objects::requirement_summary(state, id, q.revisions.last().unwrap()),
            )
        })
        .collect();
    Value::Object(summaries)
}
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

pub async fn list(pool: &Pool, actor: &str) -> Result<Value> {
    let conn = pool.get().await?;
    let allowed = access::scope(&**conn, actor).await?;
    let rows = conn
        .query(
            &format!("{} ORDER BY workspace", access::MEMBERSHIP),
            &[&actor],
        )
        .await?;
    let mut items = vec![];
    for row in rows {
        let id: String = row.get(0);
        if allowed.as_ref().is_some_and(|set| !set.contains(&id)) {
            continue;
        }
        if let Some(card) = conn.query_opt("SELECT first_view->>'workspace_id',first_view->>'title' FROM joppa.board_cache WHERE workspace=$1 AND first_view->'members' ? $2", &[&id,&actor]).await? {
            items.push(json!({"id": card.get::<_,String>(0), "title": card.get::<_,String>(1)}));
        }
    }
    Ok(json!(items))
}

/// The deployment caller supplies the exact target. Accepted staging evidence
/// must never authorize a different production build or provider configuration.
pub fn gate(card: &Value, subject: &std::collections::BTreeMap<String, String>) -> Result<()> {
    ensure!(
        card["launch"]["ready"] == true,
        "launch conditions are absent or not accepted"
    );
    let state: Workspace = serde_json::from_value(card["state"].clone())?;
    for key in crate::objects::launch_objects(&state) {
        let (_, r) = crate::objects::verification_context(&state, key)
            .context("launch condition has no verification context")?;
        let (_, run) = checks::latest(r).ok_or_else(|| anyhow::anyhow!("no verified subject"))?;
        ensure!(
            run.subject
                .iter()
                .all(|(key, value)| subject.get(key) == Some(value)),
            "launch target differs from the verified subject for {}",
            r.title
        );
    }
    Ok(())
}

/// Resolve a globally unique reference without exposing another workspace's content.
pub async fn read_item(pool: &Pool, id: &str, actor: &str) -> Result<Value> {
    read_item_scoped(pool, id, actor, Scope::from_args(&json!({}))?).await
}

pub async fn read_item_scoped(
    pool: &Pool,
    id: &str,
    actor: &str,
    mut scope: Scope,
) -> Result<Value> {
    ensure!(
        scope.req.is_none() && scope.target.is_none(),
        "item_id cannot be combined with req or target"
    );
    let conn = pool.get().await?;
    let row = conn
        .query_opt(
            "SELECT workspace,kind,local_id FROM joppa.item_ids WHERE id=$1",
            &[&id],
        )
        .await?
        .ok_or(AccessDenied)?;
    let workspace: String = row.get(0);
    let kind: String = row.get(1);
    let local_id: String = row.get(2);
    access::check(&**conn, &workspace, actor, false).await?;
    drop(conn);
    let address = format!("{kind}:{local_id}");
    let mut result = read_objects(
        pool,
        &workspace,
        actor,
        &ObjectScope {
            address: Some(&address),
            ..Default::default()
        },
    )
    .await?;
    if let Some(req) = result["process"]["req"].as_str() {
        scope.req = Some(req.into());
        let mut evidence = read(pool, &workspace, actor, &scope).await?;
        evidence["object"] = if scope.revision.is_some()
            || scope.history
            || result["document"]["present"] == false
        {
            json!({"address":result["address"],"kind":result["kind"],"id":result["id"],"item_id":result["item_id"],"current_revision":result["current_revision"],"revision_count":result["revision_count"],"metadata_scope":"current"})
        } else {
            result.clone()
        };
        if scope.revision.is_none() && !scope.history {
            evidence["process"] = result["process"].clone();
        }
        let r = &evidence["requirement"]["revisions"][0];
        let criterion = result["process"]["criterion"].as_str();
        evidence["selection"] = json!({"kind":kind,"id":local_id,"system_id":id,"req":req,"revision":r["number"],"present":criterion.is_none_or(|ac|r["acs"].as_array().is_some_and(|acs|acs.iter().any(|a|a["id"]==ac)))});
        return Ok(evidence);
    }
    if scope.revision.is_some() || scope.history {
        result = read_objects(
            pool,
            &workspace,
            actor,
            &ObjectScope {
                address: Some(&address),
                revision: scope.revision,
                history: scope.history,
                ..Default::default()
            },
        )
        .await?;
    }
    result["selection"] = json!({"kind":result["kind"],"id":result["id"],"system_id":id});
    Ok(result)
}

#[path = "search.rs"]
pub mod search;

/// Предел одного вложения. Количество не ограничено — их столько, сколько
/// нужно; ограничен РАЗМЕР, потому что байты лежат в той же базе, что и
/// журнал, и её резервная копия снимается целиком.
pub const ATTACHMENT_MAX: usize = 10 * 1024 * 1024;

/// Положить файл и вернуть его отпечаток.
///
/// Отпечаток — sha256 содержимого, поэтому повторная загрузка того же файла
/// ничего не добавляет: она возвращает тот же ключ. Проверки доступа здесь
/// нет намеренно — хранилище не знает про воркспейсы; правом распоряжается
/// то место, где отпечаток попадает в журнал.
pub async fn put_blob(pool: &Pool, bytes: &[u8]) -> Result<(String, usize)> {
    use sha2::{Digest, Sha256};
    ensure!(!bytes.is_empty(), "an empty file carries nothing");
    ensure!(
        bytes.len() <= ATTACHMENT_MAX,
        "an attachment is at most {} MB",
        ATTACHMENT_MAX / (1024 * 1024)
    );
    let hash = format!("{:x}", Sha256::digest(bytes));
    pool.get()
        .await?
        .execute(
            "INSERT INTO joppa.blobs(hash,bytes,size) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
            &[&hash, &bytes, &(bytes.len() as i64)],
        )
        .await?;
    Ok((hash, bytes.len()))
}

/// Выдать файл — но только тому, кто видит объект, к которому он приложен.
///
/// Отпечаток глобален: один и тот же файл может лежать в двух воркспейсах.
/// Поэтому право проверяется не по файлу, а по УПОМИНАНИЮ: этот отпечаток
/// должен встречаться в заметках ЭТОГО воркспейса. Иначе знание отпечатка
/// было бы паролем, а он вычисляется из содержимого.
pub async fn get_blob(
    pool: &Pool,
    workspace: &str,
    actor: &str,
    hash: &str,
) -> Result<(String, String, Vec<u8>)> {
    let conn = pool.get().await?;
    let resolved = resolve_workspace(&**conn, workspace).await?;
    access::check(&**conn, &resolved, actor, false).await?;
    let found = conn.query_opt("SELECT a.value->>'name',a.value->>'content_type' FROM joppa.state_entries n CROSS JOIN LATERAL jsonb_each(n.value->'attachments') a WHERE n.workspace=$1 AND n.section='notes' AND a.value->>'hash'=$2 LIMIT 1", &[&resolved,&hash]).await?.ok_or(AccessDenied)?;
    let row = conn
        .query_opt("SELECT bytes FROM joppa.blobs WHERE hash=$1", &[&hash])
        .await?
        .ok_or(AccessDenied)?;
    Ok((found.get(0), found.get(1), row.get(0)))
}
