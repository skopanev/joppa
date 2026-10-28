//! Пояснения, обсуждение и вложения — на любом объекте.
// Этому набору нужна только подготовка мира; проверки зелёного живут в model.rs.
mod common;
use common::*;
use serde_json::json;

/// Адрес проверяется на существование объекта, а не только на форму.
/// Заметка на несуществующем домене не видна нигде и не находится ничем:
/// она молча уезжает в карту по ключу, который никто не спросит.
#[test]
fn an_address_must_name_something_that_exists() {
    let mut w = World::new();
    for bad in [
        "domain",         // нет двоеточия
        "domain:missing", // нет такого домена
        "capability:missing",
        "requirement:missing",
        "ac:r",         // нет косой черты
        "ac:r/missing", // нет такого условия
        "ac:missing/ios",
        "intake:missing",
        "point:missing/1",
        "workspace:x", // неизвестный вид
    ] {
        w.no(
            "owner",
            json!({"op":"note","target":bad,"id":"n1","text":"hello"}),
        );
    }
    for good in ["domain:d", "capability:c", "requirement:r", "ac:r/ios"] {
        w.ok(
            "owner",
            json!({"op":"note","target":good,"id":format!("n-{}", good.replace([':','/'],"-")),"text":"hello"}),
        );
    }
}

/// Комментарий несёт автора и время ИЗ ЖУРНАЛА. Клиент не присылает ни того,
/// ни другого: по этим отметкам потом восстанавливают, кто что знал и когда.
#[test]
fn a_comment_carries_the_journal_author_and_time() {
    let mut w = World::new();
    w.ok(
        "agent",
        json!({"op":"note","target":"domain:d","id":"n1","text":"Boundary is unclear"}),
    );
    let note = &w.state.notes["domain:d"].comments["n1"];
    assert_eq!(note.actor, "agent");
    assert_eq!(note.at, "2026-01-01T00:00:00Z");
    assert!(!note.withdrawn);

    // Один и тот же идентификатор дважды — отказ, иначе вторая реплика
    // молча затирает первую.
    w.no(
        "owner",
        json!({"op":"note","target":"domain:d","id":"n1","text":"Mine now"}),
    );
    // Ответить можно только на существующую реплику.
    w.no(
        "owner",
        json!({"op":"note","target":"domain:d","id":"n2","text":"re","reply_to":"nope"}),
    );
    // Отзывает только автор, и текст остаётся: отозвано — не значит стёрто.
    w.no(
        "owner",
        json!({"op":"withdraw_note","target":"domain:d","id":"n1","reason":"no"}),
    );
    w.ok(
        "agent",
        json!({"op":"withdraw_note","target":"domain:d","id":"n1","reason":"wrong place"}),
    );
    let note = &w.state.notes["domain:d"].comments["n1"];
    assert!(note.withdrawn);
    assert_eq!(note.text, "Boundary is unclear");
}

/// Описание домена и капабилити живёт в них самих — они не версионируются.
/// У условия приёмки своё описание принадлежит РЕВИЗИИ, и пояснение ложится
/// рядом заметкой, не переписывая утверждённый текст.
#[test]
fn a_description_lands_where_the_object_keeps_it() {
    let mut w = World::new();
    w.ok(
        "owner",
        json!({"op":"describe","target":"domain:d","text":"Payments are elsewhere","reason":"boundary"}),
    );
    assert_eq!(w.state.domains["d"].description, "Payments are elsewhere");
    assert!(!w.state.notes.contains_key("domain:d"));

    let before = w.state.requirements["r"].revisions[0].acs[0]
        .description
        .clone();
    w.ok(
        "owner",
        json!({"op":"describe","target":"ac:r/ios","text":"Reinstall is out of scope","reason":"clarify"}),
    );
    assert_eq!(
        w.state.notes["ac:r/ios"].description,
        "Reinstall is out of scope"
    );
    assert_eq!(
        w.state.requirements["r"].revisions[0].acs[0].description, before,
        "a confirmed revision is never rewritten in place"
    );

    // The limit is 1024 characters, not bytes: Cyrillic text gets the same room as Latin.
    let at_limit = "я".repeat(1024);
    w.ok(
        "owner",
        json!({"op":"describe","target":"requirement:r","text":at_limit,"reason":"long"}),
    );
    w.no(
        "owner",
        json!({"op":"describe","target":"requirement:r","text":"x".repeat(1025),"reason":"long"}),
    );
}

/// Вложение — только про файл; самого файла в журнале нет. Снятие не удаляет
/// байты: тот же файл мог быть приложен и в другом месте, а ссылка из истории
/// должна открываться.
#[test]
fn an_attachment_records_the_file_and_detaching_keeps_it() {
    let mut w = World::new();
    let hash = "a".repeat(64);
    let attach = |id: &str, hash: &str| json!({"op":"attach","target":"requirement:r","id":id,"name":"spec.pdf","content_type":"application/pdf","size":1024,"hash":hash});
    w.no("owner", attach("f1", "not-a-digest"));
    w.no("owner", attach("f1", &"A".repeat(64)));
    w.no(
        "owner",
        json!({"op":"attach","target":"requirement:r","id":"f1","name":"x","content_type":"","size":0,"hash":hash}),
    );
    w.ok("owner", attach("f1", &hash));
    w.no("owner", attach("f1", &hash));

    let a = &w.state.notes["requirement:r"].attachments["f1"];
    assert_eq!(a.actor, "owner");
    assert_eq!(a.content_type, "application/pdf");
    assert!(!a.removed);

    w.ok(
        "owner",
        json!({"op":"detach","target":"requirement:r","id":"f1","reason":"wrong file"}),
    );
    assert!(w.state.notes["requirement:r"].attachments["f1"].removed);
    w.no(
        "owner",
        json!({"op":"detach","target":"requirement:r","id":"nope","reason":"x"}),
    );
}

/// The root index omits discussion text. One address returns its annotations.
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL (database name must start with joppa_test)"]
async fn the_index_carries_roots_and_one_object_carries_annotations() -> anyhow::Result<()> {
    use joppa::{commands::Command, store};
    let pool = store::pool(&std::env::var("JOPPA_TEST_DATABASE_URL")?)?;
    store::migrate(&pool).await?;
    let ws = format!(
        "notes{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let mut n = 0;
    let mut run = |v: serde_json::Value| {
        n += 1;
        let id = format!("{ws}-{n}");
        let pool = pool.clone();
        let ws = ws.clone();
        async move {
            store::execute(
                &pool,
                &ws,
                "prober",
                &id,
                &serde_json::from_value::<Command>(v)?,
            )
            .await
        }
    };
    run(json!({"op":"workspace","title":"Notes probe"})).await?;
    run(json!({"op":"domain","id":"billing","title":"Billing","authority":"prober"})).await?;
    run(json!({"op":"capability","id":"invoice","title":"Invoices","domain":"billing"})).await?;
    run(json!({"op":"consumer","id":"buyer","title":"Buyer","owner":"prober"})).await?;
    run(json!({"op":"revise","req":"r1","expected_revision":0,"capability":"invoice","consumers":["buyer"],"title":"Invoice is delivered","body":"","owner":"prober","reason":"first","acs":[{"id":"pdf","text":"A PDF is produced"}]})).await?;
    run(json!({"op":"note","target":"domain:billing","id":"n1","text":"Who owns refunds?"}))
        .await?;
    run(json!({"op":"note","target":"domain:billing","id":"n2","text":"Not us."})).await?;
    run(json!({"op":"attach","target":"domain:billing","id":"f1","name":"chart.png","content_type":"image/png","size":10,"hash":"b".repeat(64)})).await?;
    run(json!({"op":"withdraw_note","target":"domain:billing","id":"n2","reason":"wrong"})).await?;

    let scope = store::Scope::from_args(&json!({}))?;
    let index = store::read(&pool, &ws, "prober", &scope).await?;
    let roots = index["index"]["objects"].as_array().unwrap();
    assert_eq!(roots.len(), 2);
    assert!(roots.iter().all(|item| item["parent"].is_null()));
    assert!(roots.iter().any(|item| item["address"] == "domain:billing"));
    assert!(roots.iter().all(|item| item.get("data").is_none()
        && item.get("annotations").is_none()
        && item.get("comments").is_none()));
    assert!(!index.to_string().contains("Who owns refunds?"));

    let one = store::read(
        &pool,
        &ws,
        "prober",
        &store::Scope::from_args(&json!({"target":"domain:billing"}))?,
    )
    .await?;
    assert_eq!(
        one["notes"]["comments"]["n1"]["text"],
        json!("Who owns refunds?")
    );
    assert_eq!(
        one["notes"]["attachments"]["f1"]["name"],
        json!("chart.png")
    );
    assert!(
        one.get("index").is_none(),
        "one object is not the whole tree"
    );

    let bad = store::read(
        &pool,
        &ws,
        "prober",
        &store::Scope::from_args(&json!({"target":"domain:missing"}))?,
    )
    .await;
    assert!(
        bad.is_err(),
        "an unknown address is refused, not answered with emptiness"
    );
    Ok(())
}
