//! joppa — приём (intake).
//!
//! Одно свойство, ради которого всё это существует: **intake нельзя отвергнуть**.
//! Человек пишет как сказал, и это уже законная запись. Отказывают не ей, а
//! готовности — тому, что из неё выросло. Поэтому здесь нет ни одной проверки,
//! которая может не принять текст человека.
//!
//! Второе: список вопросов лежит в базе, а не в описаниях инструментов. Меняется
//! политика приёма — правится строка в `needs`, а не промпт и не этот файл.

use anyhow::Result;
use deadpool_postgres::Pool;
use serde_json::{json, Value};
use std::io::Write;

mod board;
mod tools;

const DEFAULT_URL: &str = "postgresql://postgres:joppa@127.0.0.1:55433/joppa";

pub async fn run() -> Result<()> {
    let url = std::env::var("JOPPA_DATABASE_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
    let pool = pool(&url)?;

    // Один бинарь и две роли: без аргументов — сервер MCP по stdio, с `board` —
    // разовая отрисовка страницы. Отдельная команда нужна, чтобы доску можно
    // было перерисовать из шелла, не поднимая сессию.
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.first().map(String::as_str) == Some("serve") {
        let port: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6212);
        // Адрес задаётся явно: по умолчанию петля, наружу — только по просьбе.
        let host = args.get(2).cloned().unwrap_or_else(|| "127.0.0.1".to_string());
        return serve(pool, &host, port).await;
    }
    if args.first().map(String::as_str) == Some("board") {
        let path = args.get(1).cloned().unwrap_or_else(default_board_path);
        let html = board::snapshot(&pool).await?;
        std::fs::write(&path, html)?;
        println!("{path}");
        return Ok(());
    }

    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                reply(&json!({
                    "jsonrpc": "2.0", "id": Value::Null,
                    "error": {"code": -32700, "message": format!("не разобрал запрос: {e}")}
                }))?;
                continue;
            }
        };
        if let Some(out) = handle(&pool, &msg).await {
            reply(&out)?;
        }
    }
}

/// Живая доска.
async fn serve(pool: Pool, host: &str, port: u16) -> Result<()> {
    use axum::{extract::State, response::Html, routing::get, Router};

    async fn full(State(p): State<Pool>) -> Html<String> {
        Html(board::render(&p).await.unwrap_or_else(|e| format!("<pre>{e}</pre>")))
    }
    async fn part(State(p): State<Pool>) -> Html<String> {
        Html(board::fragment(&p).await.unwrap_or_else(|e| format!("<pre>{e}</pre>")))
    }

    let app = Router::new()
        .route("/", get(full))
        .route("/body", get(part))
        .with_state(pool);
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("доска: http://{host}:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn default_board_path() -> String {
    match std::env::var("HOME") {
        Ok(h) => format!("{h}/Downloads/joppa-board.html"),
        Err(_) => "joppa-board.html".to_string(),
    }
}

fn pool(url: &str) -> Result<Pool> {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.url = Some(url.to_string());
    cfg.pool = Some(deadpool_postgres::PoolConfig { max_size: 4, ..Default::default() });
    Ok(cfg.create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls)?)
}

fn reply(v: &Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{v}")?;
    out.flush()?;
    Ok(())
}

/// Возвращает `None` для уведомлений: на них по протоколу не отвечают.
async fn handle(pool: &Pool, msg: &Value) -> Option<Value> {
    let method = msg.get("method")?.as_str()?;
    let id = msg.get("id").cloned();

    // Уведомление — это сообщение без id. Ответ на него был бы протокольной ошибкой.
    if id.is_none() {
        return None;
    }
    let id = id.unwrap();

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "joppa", "version": env!("CARGO_PKG_VERSION")},
            "instructions": tools::INSTRUCTIONS,
        })),
        "tools/list" => Ok(json!({"tools": tools::catalogue()})),
        "tools/call" => {
            let name = msg.pointer("/params/name").and_then(Value::as_str).unwrap_or("");
            let args = msg.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
            call(pool, name, &args).await
        }
        "ping" => Ok(json!({})),
        other => Err(anyhow::anyhow!("не знаю метода {other}")),
    };

    Some(match result {
        Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
        // Ошибка инструмента отдаётся как результат с isError, а не как ошибка
        // протокола: агент должен её прочитать и исправиться, а не считать,
        // что сервер сломался.
        Err(e) if method == "tools/call" => json!({
            "jsonrpc": "2.0", "id": id,
            "result": {"content": [{"type": "text", "text": e.to_string()}], "isError": true}
        }),
        Err(e) => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32603, "message": e.to_string()}
        }),
    })
}

fn text(s: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": s.into()}]})
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("не хватает поля {key}"))
}

async fn call(pool: &Pool, name: &str, args: &Value) -> Result<Value> {
    match name {
        "joppa_file" => file(pool, args).await,
        "joppa_split" => split(pool, args).await,
        "joppa_sort" => sort(pool, args).await,
        "joppa_answer" => answer(pool, args).await,
        "joppa_drop" => drop_item(pool, args).await,
        "joppa_show" => show(pool, args).await,
        "joppa_ls" => ls(pool, args).await,
        "joppa_tree" => tree(pool).await,
        "joppa_place" => place(pool, args).await,
        "joppa_board" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(default_board_path);
            let html = board::snapshot(pool).await?;
            std::fs::write(&path, html)?;
            Ok(text(format!("Доска перерисована: {path}\n\nОткройте файл в браузере; \
                             при повторном вызове обновится по тому же пути.")))
        }
        other => Err(anyhow::anyhow!("нет такого инструмента: {other}")),
    }
}

/// Номер по порядку: i-1, i-2. Случайные восемь знаков не диктуются голосом и
/// не ищутся глазами — а этот номер люди будут называть друг другу вслух.
async fn new_id(pool: &Pool) -> Result<String> {
    let c = pool.get().await?;
    let row = c.query_one("select 'i-' || nextval('intake_no')", &[]).await?;
    Ok(row.get(0))
}

async fn file(pool: &Pool, args: &Value) -> Result<Value> {
    let said = arg(args, "said")?;
    let author = arg(args, "author")?;
    let id = new_id(pool).await?;
    let c = pool.get().await?;
    c.execute(
        "insert into intake(id, said, author) values ($1, $2, $3)",
        &[&id, &said, &author],
    )
    .await?;
    Ok(text(format!(
        "Принято: {id}\n\nЗаписано дословно. Теперь разберите intake на пункты через joppa_split. \
         Если просьба одна — один пункт, это нормально. Род и вопросы назначаются ПУНКТУ, \
         потому что у пунктов одного intake судьба разная: что-то делаем, что-то не делаем совсем."
    )))
}

async fn split(pool: &Pool, args: &Value) -> Result<Value> {
    let id = arg(args, "id")?;
    let points = args
        .get("points")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| anyhow::anyhow!("не хватает points: список пунктов"))?;

    let mut c = pool.get().await?;
    let tx = c.transaction().await?;

    let exists: i64 = tx
        .query_one("select count(*) from intake where id = $1", &[&id])
        .await?
        .get(0);
    if exists == 0 {
        return Err(anyhow::anyhow!("intake {id} нет"));
    }
    let already: i64 = tx
        .query_one("select count(*) from items where intake_id = $1", &[&id])
        .await?
        .get(0);
    if already > 0 {
        return Err(anyhow::anyhow!(
            "intake {id} уже разобран на {already} пунктов — смотрите joppa_show"
        ));
    }

    let mut out = format!("Intake {id} разобран на {} пунктов:\n\n", points.len());
    for (i, p) in points.iter().enumerate() {
        let said = p
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("пункт {} пустой", i + 1))?;
        let ord = (i + 1) as i32;
        let item_id = format!("{id}.{ord}");
        tx.execute(
            "insert into items(id, intake_id, ord, said) values ($1,$2,$3,$4)",
            &[&item_id, &id, &ord, &said],
        )
        .await?;
        let short: String = said.chars().take(60).collect();
        out.push_str(&format!("  {item_id}  {short}\n"));
    }
    tx.commit().await?;
    out.push_str(
        "\nДальше по КАЖДОМУ пункту: joppa_sort, если он идёт в работу, \
         или joppa_drop, если его решили не делать.",
    );
    Ok(text(out))
}

async fn sort(pool: &Pool, args: &Value) -> Result<Value> {
    let id = arg(args, "id")?;
    let kind = arg(args, "kind")?;
    let c = pool.get().await?;

    let known: i64 = c
        .query_one("select count(*) from needs where kind = $1", &[&kind])
        .await?
        .get(0);
    if known == 0 {
        let kinds: Vec<String> = c
            .query("select distinct kind from needs order by 1", &[])
            .await?
            .iter()
            .map(|r| r.get::<_, String>(0))
            .collect();
        return Err(anyhow::anyhow!("рода «{kind}» нет. Известные: {}", kinds.join(", ")));
    }

    let outcome: Option<String> = c
        .query_opt("select outcome from items where id = $1", &[&id])
        .await?
        .ok_or_else(|| anyhow::anyhow!("пункта {id} нет. Сначала joppa_split"))?
        .get(0);
    if let Some(o) = outcome {
        if o != "work" {
            return Err(anyhow::anyhow!(
                "пункт {id} решено не делать ({o}) — вопросы по нему задавать незачем"
            ));
        }
    }

    c.execute(
        "update items set kind = $2, kind_at = now(),
                outcome = 'work', outcome_by = coalesce(outcome_by, 'разбор'),
                outcome_at = coalesce(outcome_at, now())
           where id = $1",
        &[&id, &kind],
    )
    .await?;
    missing_report(pool, id).await
}

async fn drop_item(pool: &Pool, args: &Value) -> Result<Value> {
    let id = arg(args, "id")?;
    let why = arg(args, "why")?;
    let by = arg(args, "by")?;
    let outcome = args
        .get("outcome")
        .and_then(Value::as_str)
        .unwrap_or("refused");
    if outcome != "refused" && outcome != "elsewhere" {
        return Err(anyhow::anyhow!(
            "outcome бывает refused (не делаем) или elsewhere (решается иначе)"
        ));
    }
    let c = pool.get().await?;
    let n = c
        .execute(
            "update items set outcome = $2, outcome_why = $3, outcome_by = $4, outcome_at = now()
               where id = $1",
            &[&id, &outcome, &why, &by],
        )
        .await?;
    if n == 0 {
        return Err(anyhow::anyhow!("пункта {id} нет"));
    }
    let word = if outcome == "refused" { "не делаем" } else { "решается иначе" };
    Ok(text(format!(
        "{id}: {word}.\n  Причина: {why}\n  Решил: {by}\n\nПричина сохранена — \
         через месяц будет видно, почему не сделали, и пункт не заведут заново."
    )))
}

async fn answer(pool: &Pool, args: &Value) -> Result<Value> {
    let id = arg(args, "id")?;
    let field = arg(args, "field")?;
    let value = arg(args, "value")?;
    let by = arg(args, "answered_by")?;
    let c = pool.get().await?;

    let kind: Option<String> = c
        .query_opt("select kind from items where id = $1", &[&id])
        .await?
        .ok_or_else(|| anyhow::anyhow!("пункта {id} нет"))?
        .get(0);
    let kind = kind
        .ok_or_else(|| anyhow::anyhow!("у пункта {id} не определён род — сначала joppa_sort"))?;

    let asked: i64 = c
        .query_one(
            "select count(*) from needs where kind = $1 and field = $2",
            &[&kind, &field],
        )
        .await?
        .get(0);
    if asked == 0 {
        return Err(anyhow::anyhow!(
            "роду «{kind}» такой вопрос не задают: {field}. Смотрите joppa_show"
        ));
    }

    c.execute(
        "insert into answers(item_id, field, value, answered_by) values ($1,$2,$3,$4)
         on conflict (item_id, field) do update
            set value = excluded.value, answered_by = excluded.answered_by, answered_at = now()",
        &[&id, &field, &value, &by],
    )
    .await?;
    missing_report(pool, id).await
}

async fn missing_report(pool: &Pool, id: &str) -> Result<Value> {
    let c = pool.get().await?;
    let rows = c
        .query(
            "select n.field, n.ask, n.why, n.required
               from items i
               join needs n on n.kind = i.kind
               left join answers a on a.item_id = i.id and a.field = n.field
              where i.id = $1 and a.field is null
              order by n.required desc, n.ord",
            &[&id],
        )
        .await?;

    let mut must = Vec::new();
    let mut may = Vec::new();
    for r in &rows {
        let field: String = r.get(0);
        let ask: String = r.get(1);
        let why: Option<String> = r.get(2);
        let line = match why {
            Some(w) => format!("  {field}: {ask}\n      — {w}"),
            None => format!("  {field}: {ask}"),
        };
        if r.get::<_, bool>(3) { must.push(line) } else { may.push(line) }
    }

    let mut out = format!("Пункт {id}\n");
    if must.is_empty() {
        out.push_str("Обязательное собрано. Пункт можно раскладывать.\n");
        if !may.is_empty() {
            out.push_str("\nНеобязательное, спрашивайте только если человек сам заговорит:\n");
            out.push_str(&may.join("\n"));
        }
    } else {
        out.push_str("Спросите у автора — и только это:\n");
        out.push_str(&must.join("\n"));
        out.push_str("\n\nОтветы записывайте через joppa_answer. Технических вопросов автору не задавайте.");
    }
    Ok(text(out))
}

async fn show(pool: &Pool, args: &Value) -> Result<Value> {
    let id = arg(args, "id")?;
    let c = pool.get().await?;
    let row = c
        .query_opt(
            "select said, author, to_char(said_at,'YYYY-MM-DD HH24:MI') from intake where id = $1",
            &[&id],
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("intake {id} нет"))?;

    let said: String = row.get(0);
    let author: String = row.get(1);
    let at: String = row.get(2);
    let mut out = format!("{id}  {at}  от {author}\n\nСказано дословно:\n  {said}\n");

    let items = c
        .query(
            "select id, said, kind, outcome, outcome_why, outcome_by,
                    (select count(*) from needs n
                      left join answers a on a.item_id = i.id and a.field = n.field
                     where n.kind = i.kind and n.required and a.field is null)
               from items i where intake_id = $1 order by ord",
            &[&id],
        )
        .await?;
    if items.is_empty() {
        out.push_str("\nНа пункты не разобран — joppa_split\n");
        return Ok(text(out));
    }

    out.push_str("\nПункты:\n");
    for r in &items {
        let iid: String = r.get(0);
        let isaid: String = r.get(1);
        let kind: Option<String> = r.get(2);
        let outcome: Option<String> = r.get(3);
        let why: Option<String> = r.get(4);
        let by: Option<String> = r.get(5);
        let left: Option<i64> = r.get(6);
        let short: String = isaid.chars().take(64).collect();
        let state = match (outcome.as_deref(), &kind, left) {
            (Some("refused"), _, _) => format!(
                "НЕ ДЕЛАЕМ — {} ({})",
                why.unwrap_or_default(),
                by.unwrap_or_default()
            ),
            (Some("elsewhere"), _, _) => format!(
                "РЕШАЕТСЯ ИНАЧЕ — {} ({})",
                why.unwrap_or_default(),
                by.unwrap_or_default()
            ),
            (_, Some(k), Some(n)) if n > 0 => format!("{k}, не хватает {n}"),
            (_, Some(k), _) => format!("{k}, собрано"),
            _ => "род не определён".to_string(),
        };
        out.push_str(&format!("\n  {iid}  [{state}]\n    {short}\n"));
    }

    let answers = c
        .query(
            "select a.item_id, a.field, a.value from answers a
               join items i on i.id = a.item_id
              where i.intake_id = $1 order by i.ord, a.field",
            &[&id],
        )
        .await?;
    if !answers.is_empty() {
        out.push_str("\nОтветы:\n");
        for r in &answers {
            out.push_str(&format!(
                "  {} {}: {}\n",
                r.get::<_, String>(0),
                r.get::<_, String>(1),
                r.get::<_, String>(2)
            ));
        }
    }
    Ok(text(out))
}

/// Дерево целиком: куда вообще можно положить пункт.
async fn tree(pool: &Pool) -> Result<Value> {
    let c = pool.get().await?;
    let rows = c
        .query(
            "with recursive walk as (
               select id, kind, parent_id, title, 0 as depth, title as path
                 from nodes where parent_id is null
               union all
               select n.id, n.kind, n.parent_id, n.title, w.depth + 1, w.path || ' / ' || n.title
                 from nodes n join walk w on n.parent_id = w.id)
             select id, kind, title, depth from walk order by path",
            &[],
        )
        .await?;
    if rows.is_empty() {
        return Ok(text("Дерево пустое."));
    }
    let mut out = String::new();
    for r in &rows {
        let depth: i32 = r.get(3);
        out.push_str(&format!(
            "{}{}  [{}]  {}\n",
            "  ".repeat(depth as usize),
            r.get::<_, String>(0),
            r.get::<_, String>(1),
            r.get::<_, String>(2)
        ));
    }
    out.push_str("\nПункт кладётся в дерево через joppa_place: либо к существующему \
                  требованию, либо новым требованием под капабилити.");
    Ok(text(out))
}

/// Положить пункт в дерево: к существующему требованию или новым под капабилити.
async fn place(pool: &Pool, args: &Value) -> Result<Value> {
    let item = arg(args, "id")?;
    let mut c = pool.get().await?;
    let tx = c.transaction().await?;

    let outcome: Option<String> = tx
        .query_opt("select outcome from items where id = $1", &[&item])
        .await?
        .ok_or_else(|| anyhow::anyhow!("пункта {item} нет"))?
        .get(0);
    if let Some(o) = outcome.as_deref() {
        if o != "work" {
            return Err(anyhow::anyhow!(
                "пункт {item} решено не делать ({o}) — в дерево он не кладётся"
            ));
        }
    }

    let node = match (args.get("node").and_then(Value::as_str), args.get("under").and_then(Value::as_str)) {
        // К существующему требованию: так ложатся баги — нового требования не появляется.
        (Some(n), None) => {
            let kind: String = tx
                .query_opt("select kind from nodes where id = $1", &[&n])
                .await?
                .ok_or_else(|| anyhow::anyhow!("узла {n} нет — смотрите joppa_tree"))?
                .get(0);
            if kind != "requirement" {
                return Err(anyhow::anyhow!(
                    "к узлу {n} ({kind}) пункт не привязывается: работа ведётся ради ТРЕБОВАНИЯ. \
                     Чтобы завести новое, передайте under = капабилити и title"
                ));
            }
            n.to_string()
        }
        // Новое требование под капабилити.
        (None, Some(u)) => {
            let title = arg(args, "title")?;
            let kind: String = tx
                .query_opt("select kind from nodes where id = $1", &[&u])
                .await?
                .ok_or_else(|| anyhow::anyhow!("узла {u} нет — смотрите joppa_tree"))?
                .get(0);
            if kind != "capability" {
                return Err(anyhow::anyhow!(
                    "требование живёт под капабилити, а {u} это {kind}"
                ));
            }
            let id: String = tx
                .query_one("select 'r-' || nextval('intake_no')", &[])
                .await?
                .get(0);
            tx.execute(
                "insert into nodes(id, kind, parent_id, title) values ($1,'requirement',$2,$3)",
                &[&id, &u, &title],
            )
            .await?;
            id
        }
        _ => {
            return Err(anyhow::anyhow!(
                "передайте либо node (существующее требование), либо under + title (новое требование под капабилити)"
            ))
        }
    };

    tx.execute(
        "update items set node_id = $2, outcome = 'work',
                outcome_by = coalesce(outcome_by,'разбор'), outcome_at = coalesce(outcome_at, now())
           where id = $1",
        &[&item, &node],
    )
    .await?;
    let title: String = tx
        .query_one("select title from nodes where id = $1", &[&node])
        .await?
        .get(0);
    tx.commit().await?;
    Ok(text(format!("{item} → {node}\n  {title}")))
}

async fn ls(pool: &Pool, args: &Value) -> Result<Value> {
    let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
    let c = pool.get().await?;
    let rows = c
        .query(
            "select i.id, i.author, left(i.said, 64),
                    (select count(*) from items t where t.intake_id = i.id),
                    (select count(*) from items t where t.intake_id = i.id and t.outcome is null),
                    (select count(*) from items t
                       join needs n on n.kind = t.kind
                       left join answers a on a.item_id = t.id and a.field = n.field
                      where t.intake_id = i.id and n.required and a.field is null)
               from intake i
              order by i.said_at desc limit 50",
            &[],
        )
        .await?;

    if rows.is_empty() {
        return Ok(text("Пусто."));
    }
    let mut out = String::new();
    for r in &rows {
        let total: i64 = r.get(3);
        let undecided: i64 = r.get(4);
        let unanswered: i64 = r.get(5);
        let state = if total == 0 {
            "не разобрана".to_string()
        } else if undecided > 0 {
            format!("{total} пунктов, не решено {undecided}")
        } else if unanswered > 0 {
            format!("{total} пунктов, не хватает {unanswered} ответов")
        } else {
            format!("{total} пунктов, собрано")
        };
        if !all && total > 0 && undecided == 0 && unanswered == 0 {
            continue;
        }
        out.push_str(&format!(
            "{}  {}  [{}]\n    {}\n",
            r.get::<_, String>(0),
            r.get::<_, String>(1),
            state,
            r.get::<_, String>(2)
        ));
    }
    if out.is_empty() {
        out.push_str("Всё разобрано. all=true покажет.");
    }
    Ok(text(out))
}
