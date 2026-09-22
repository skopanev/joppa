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
pub use access::{AccessDenied, enroll};

pub const DEFAULT_URL: &str = "postgresql://postgres:joppa@127.0.0.1:55433/joppa";
pub fn pool(url: &str) -> Result<Pool> {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(url.to_string());
    Ok(cfg.create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?)
}
pub async fn migrate(pool: &Pool) -> Result<()> {
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
    tx.commit().await?;
    Ok(())
}
#[derive(Serialize)]
pub struct Event {
    pub position: u64,
    pub actor: String,
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
        commands::apply(&mut state, &actor, &command, position, row.get(2))
            .with_context(|| format!("invalid journal entry at {workspace}/{position}"))?;
        events.push(Event {
            position,
            actor,
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
#[path = "store_write.rs"]
mod write;
pub use write::{BatchOperation, execute, execute_batch, execute_input};
fn summaries(state: &Workspace) -> Value {
    let summaries: serde_json::Map<String, Value> = state
        .requirements
        .iter()
        .map(|(id, q)| (id.clone(), checks::summary(q.revisions.last().unwrap())))
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
    /// Полный ответ, как раньше. Нужен доске в браузере: она не ограничена
    /// контекстом, и деление на страницы там только мешало бы.
    pub full: bool,
    /// Одно требование целиком: все ревизии, AC, задачи, прогоны, находки.
    pub req: Option<String>,
    /// Историю по умолчанию НЕ отдаём: она самая большая часть и нужна реже
    /// всего. Число событий видно и без неё.
    pub history: bool,
    /// Сколько событий истории за раз.
    pub limit: usize,
    /// Непрозрачная отметка места. Непрозрачная не для красоты: клиент не
    /// должен на неё опираться, иначе её нельзя будет поменять.
    pub cursor: Option<String>,
}
impl Scope {
    pub fn full() -> Self {
        Self { full: true, req: None, history: true, limit: usize::MAX, cursor: None }
    }
    pub fn from_args(a: &Value) -> Result<Self> {
        let limit = a["limit"].as_u64().unwrap_or(50).clamp(1, 200) as usize;
        Ok(Self {
            full: false,
            req: a["req"].as_str().map(str::to_string),
            history: a["history"].as_bool().unwrap_or(false),
            limit,
            cursor: a["cursor"].as_str().map(str::to_string),
        })
    }
}

/// Курсор — это позиция события, закодированная так, чтобы её не читали
/// глазами и не подставляли руками. Состояния на сервере он не заводит:
/// хранимые курсоры пришлось бы чистить, а этот живёт в самой строке.
fn encode_cursor(position: u64) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("p{position}"))
}
fn decode_cursor(cursor: &str) -> Result<u64> {
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.strip_prefix('p').and_then(|n| n.parse().ok()));
    raw.context("invalid cursor")
}

pub async fn read(pool: &Pool, workspace: &str, actor: &str, scope: &Scope) -> Result<Value> {
    let conn = pool.get().await?;
    let resolved = resolve_workspace(&**conn, workspace).await?;
    let workspace = resolved.as_str();
    access::check(&**conn, workspace, actor, false).await?;
    let (state, events) = load(&**conn, workspace).await?;
    ensure!(
        state.members.contains_key(actor),
        "workspace not found or access denied"
    );
    let mut system_ids = json!({"domain":{},"capability":{},"requirement":{},"ac":{}});
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
    let configured = state.requirements.values().any(|q| {
        q.revisions
            .last()
            .unwrap()
            .acs
            .iter()
            .any(|a| a.before_launch)
    });
    let launch_blockers: Vec<_> = state
        .requirements
        .iter()
        .flat_map(|(id, q)| {
            let r = q.revisions.last().unwrap();
            let accepted = summaries[id]["accepted"] == true;
            r.acs
                .iter()
                .filter(move |a| a.before_launch && !accepted)
                .map(move |a| json!({"req": id, "revision": r.number, "ac": a.id}))
        })
        .collect();
    let launch = json!({"ready": configured && launch_blockers.is_empty(), "configured": configured, "blockers": launch_blockers, "meaning": "Only explicitly marked before_launch ACs are evaluated; this is not a deployment command."});
    let mut out = json!({
        "workspace": state.workspace_id.as_deref().unwrap_or(workspace),
        "system_ids": system_ids,
        "position": events.len(),
        "launch": launch,
    });

    if scope.full {
        out["state"] = json!(state);
        out["summary"] = json!(summaries);
        out["history"] = json!(events);
        return Ok(out);
    }

    // Одно требование целиком — то, ради чего обычно и читают.
    if let Some(req) = &scope.req {
        let q = state
            .requirements
            .get(req)
            .with_context(|| format!("unknown requirement {req}"))?;
        out["requirement"] = json!({"id": req, "capability": q.capability, "revisions": q.revisions, "comments": q.comments});
        out["summary"] = summaries.get(req).cloned().unwrap_or(Value::Null);
        return Ok(out);
    }

    // По умолчанию — ОГЛАВЛЕНИЕ. Домены и капабилити целиком: их десятки, и
    // без них не сориентироваться. Требования — заголовком и состоянием, без
    // ревизий, AC, задач, прогонов и находок: их сотни, и каждое тянет за
    // собой столько же, сколько весь остальной ответ.
    let requirements: Vec<Value> = state
        .requirements
        .iter()
        .map(|(id, q)| {
            let r = q.revisions.last().unwrap();
            json!({
                "id": id,
                "capability": q.capability,
                "title": r.title,
                "revision": r.number,
                "confirmed": r.confirmed,
                "acs": r.acs.len(),
                "comments": q.comments.len(),
                "summary": summaries.get(id).cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    out["index"] = json!({
        "domains": state.domains,
        "capabilities": state.capabilities,
        "requirements": requirements,
        "members": state.members,
        "reading": "Requirements are listed without revisions, acceptance criteria, tasks, runs or issues. Read one with req=<id>. History is omitted unless history=true.",
    });

    if scope.history {
        let from = match &scope.cursor {
            Some(c) => decode_cursor(c)?,
            None => 0,
        };
        let page: Vec<_> = events
            .iter()
            .filter(|e| e.position > from)
            .take(scope.limit)
            .collect();
        let last = page.last().map(|e| e.position);
        out["history"] = json!(page);
        // Усечение объявляется, а не подразумевается: молча обрезанный ответ
        // читается как полный, и по нему делают выводы.
        let more = last.is_some_and(|p| events.iter().any(|e| e.position > p));
        out["history_truncated"] = json!(more);
        if let (true, Some(p)) = (more, last) {
            out["nextCursor"] = json!(encode_cursor(p));
        }
    } else {
        out["history_events"] = json!(events.len());
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
        let (w, _) = load(&**conn, &id).await?;
        if w.members.contains_key(actor) {
            items.push(json!({"id": w.workspace_id.as_deref().unwrap_or(&id), "title": w.title}));
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
    for q in state.requirements.values() {
        let r = q.revisions.last().unwrap();
        if !r.acs.iter().any(|a| a.before_launch) {
            continue;
        }
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
    drop(conn);
    // Скопированный номер разрешается В САМ ПРЕДМЕТ, а не в весь воркспейс.
    // Раньше запрос «что это за идентификатор» возвращал всё содержимое —
    // ответ в сотню тысяч знаков на вопрос про одну строку.
    let mut selection = json!({"kind":kind,"id":local_id,"system_id":id});
    let mut scope = Scope::from_args(&json!({}))?;
    match kind.as_str() {
        "requirement" => scope.req = Some(local_id.clone()),
        "ac" => {
            let conn = pool.get().await?;
            let (state, _) = load(&**conn, &workspace).await?;
            drop(conn);
            for (req, q) in &state.requirements {
                if let Some(revision) = q
                    .revisions
                    .iter()
                    .rev()
                    .find(|r| r.acs.iter().any(|ac| ac.id == local_id))
                {
                    selection["req"] = json!(req);
                    selection["revision"] = json!(revision.number);
                    scope.req = Some(req.clone());
                    break;
                }
            }
        }
        // Домен и капабилити сами по себе малы, но их содержимое — нет.
        // Оглавления довольно, чтобы увидеть, что внутри.
        _ => {}
    }
    let mut result = read(pool, &workspace, actor, &scope).await?;
    result["selection"] = selection;
    Ok(result)
}

#[path = "search.rs"]
pub mod search;
