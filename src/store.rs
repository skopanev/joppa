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
    tx.batch_execute(include_str!("../migrations/009_annotations.sql"))
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
    // Заметки в оглавлении — ЧИСЛАМИ. Иначе одно обсуждение на сто реплик
    // весит больше, чем всё дерево, и попадает в каждый ответ.
    let counts = |address: String| match state.notes.get(&address) {
        None => Value::Null,
        Some(n) => {
            let comments = n.comments.values().filter(|c| !c.withdrawn).count();
            let attachments = n.attachments.values().filter(|a| !a.removed).count();
            if comments == 0 && attachments == 0 && n.description.is_empty() {
                Value::Null
            } else {
                json!({"comments": comments, "attachments": attachments, "described": !n.description.is_empty()})
            }
        }
    };
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

    ensure!(
        scope.revision.is_none() || scope.req.is_some(),
        "revision requires req or a requirement/AC item_id"
    );

    // Return only the requested revision; history is discovery by count.
    if let Some(req) = &scope.req {
        let q = state
            .requirements
            .get(req)
            .with_context(|| format!("unknown requirement {req}"))?;
        let current = q.revisions.last().context("requirement has no revisions")?;
        out["requirement"] = json!({"id": req, "capability": q.capability,
            "revision_count": q.revisions.len(), "current_revision": current.number});
        if scope.history && scope.revision.is_none() {
            return Ok(out);
        }
        let revision = match scope.revision {
            Some(number) => q
                .revisions
                .iter()
                .find(|r| r.number == number)
                .with_context(|| format!("unknown revision {number} for requirement {req}"))?,
            None => current,
        };
        let comments: serde_json::Map<String, Value> = q
            .comments
            .iter()
            .filter(|(_, c)| c.revision == revision.number)
            .map(|(id, c)| (id.clone(), json!(c)))
            .collect();
        let mut notes = json!({});
        for (address, n) in &state.notes {
            let mine = address == &format!("requirement:{req}")
                || revision
                    .acs
                    .iter()
                    .any(|ac| address == &format!("ac:{req}/{}", ac.id));
            if mine {
                notes[address] = json!(n);
            }
        }
        out["requirement"]["revisions"] = json!([revision]);
        out["requirement"]["comments"] = json!(comments);
        out["notes"] = notes;
        out["summary"] = checks::summary(revision);
        return Ok(out);
    }

    // Один объект: всё, что на него навешано.
    if let Some(address) = &scope.target {
        commands::target(&state, address)?;
        out["target"] = json!(address);
        out["notes"] = json!(state.notes.get(address).cloned().unwrap_or_default());
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
                "revision_count": q.revisions.len(),
                "confirmed": r.confirmed,
                "acs": r.acs.len(),
                "comments": q.comments.len(),
                "notes": counts(format!("requirement:{id}")),
                "summary": summaries.get(id).cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    let domains: serde_json::Map<String, Value> = state
        .domains
        .iter()
        .map(|(id, d)| {
            let mut v = json!(d);
            v["notes"] = counts(format!("domain:{id}"));
            (id.clone(), v)
        })
        .collect();
    let capabilities: serde_json::Map<String, Value> = state
        .capabilities
        .iter()
        .map(|(id, c)| {
            let mut v = json!(c);
            v["notes"] = counts(format!("capability:{id}"));
            (id.clone(), v)
        })
        .collect();
    out["index"] = json!({
        "domains": domains,
        "capabilities": capabilities,
        "requirements": requirements,
        "members": state.members,
        "reading": "Read one requirement with req: current revision only. revision_count gives the number available; request revision=N for exactly one older revision. history=true returns counts only, never events or revision bodies.",
    });

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
    // Скопированный номер разрешается В САМ ПРЕДМЕТ, а не в весь воркспейс.
    // Раньше запрос «что это за идентификатор» возвращал всё содержимое —
    // ответ в сотню тысяч знаков на вопрос про одну строку.
    let mut selection = json!({"kind":kind,"id":local_id,"system_id":id});
    match kind.as_str() {
        "requirement" => scope.req = Some(local_id.clone()),
        "ac" => {
            let conn = pool.get().await?;
            let (state, _) = load(&**conn, &workspace).await?;
            drop(conn);
            for (req, q) in &state.requirements {
                if q.revisions
                    .iter()
                    .any(|r| r.acs.iter().any(|ac| ac.id == local_id))
                {
                    let revision = match scope.revision {
                        Some(number) => q
                            .revisions
                            .iter()
                            .find(|r| r.number == number)
                            .with_context(|| {
                                format!("unknown revision {number} for requirement {req}")
                            })?,
                        None => q.revisions.last().context("requirement has no revisions")?,
                    };
                    selection["req"] = json!(req);
                    selection["revision"] = json!(revision.number);
                    selection["present"] = json!(revision.acs.iter().any(|ac| ac.id == local_id));
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
    let (state, _) = load(&**conn, &resolved).await?;
    ensure!(
        state.members.contains_key(actor),
        "workspace not found or access denied"
    );
    let found = state
        .notes
        .values()
        .flat_map(|n| n.attachments.values())
        .find(|a| a.hash == hash)
        .ok_or(AccessDenied)?;
    let row = conn
        .query_opt("SELECT bytes FROM joppa.blobs WHERE hash=$1", &[&hash])
        .await?
        .ok_or(AccessDenied)?;
    Ok((found.name.clone(), found.content_type.clone(), row.get(0)))
}
