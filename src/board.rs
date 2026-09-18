//! Доска: одна страница, на которой видно всё, что происходит с intake.
//!
//! Статический файл, а не сервер: порт и живой процесс здесь стоили бы дороже,
//! чем перезапись файла по требованию. Перерисовать — `joppa board` или
//! инструмент joppa_board.
//!
//! Главное, что должна показывать доска: ЧЕГО ЖДЁТ каждый пункт. Не «сколько
//! всего», а «кто следующий ходит» — иначе это просто список, и по нему нельзя
//! понять, застряло что-то или нет.

use anyhow::Result;
use deadpool_postgres::Pool;
use std::collections::HashMap;

/// Полная страница — для файла и для первого запроса.
pub async fn render(pool: &Pool) -> Result<String> {
    let (body, n, open, cold, shut) = collect(pool).await?;
    Ok(page(&body, n, open, cold, shut, true))
}

/// Снимок одним файлом: его отправляют человеку, у которого нет доступа к базе.
/// Опрос выключен — иначе страница у него будет стучаться в порт, которого нет,
/// и гореть красным, как будто что-то сломано.
pub async fn snapshot(pool: &Pool) -> Result<String> {
    let (body, n, open, cold, shut) = collect(pool).await?;
    Ok(page(&body, n, open, cold, shut, false))
}

/// Только внутренность — её страница подтягивает сама, чтобы не терять прокрутку.
pub async fn fragment(pool: &Pool) -> Result<String> {
    let (body, n, open, cold, shut) = collect(pool).await?;
    Ok(format!("{}{}", tally(n, open, cold, shut), body))
}

async fn collect(pool: &Pool) -> Result<(String, usize, usize, usize, usize)> {
    let c = pool.get().await?;

    let intakes = c
        .query(
            "select id, said, author, to_char(said_at,'DD.MM HH24:MI') from intake order by said_at desc",
            &[],
        )
        .await?;

    let items = c
        .query(
            "select i.id, i.intake_id, i.said, i.kind, i.outcome, i.outcome_why, i.outcome_by, i.node_id
               from items i order by i.intake_id, i.ord",
            &[],
        )
        .await?;

    let waiting = c
        .query(
            "select t.id, n.field, n.ask
               from items t
               join needs n on n.kind = t.kind
               left join answers a on a.item_id = t.id and a.field = n.field
              where n.required and a.field is null
              order by t.id, n.ord",
            &[],
        )
        .await?;

    let answers = c
        .query(
            "select item_id, field, value, answered_by from answers order by item_id, field",
            &[],
        )
        .await?;

    // ── ДЕРЕВО ──
    let nodes = c
        .query(
            "select id, kind, parent_id, title, body, source, taken_by, done_at from nodes order by kind desc, title",
            &[],
        )
        .await?;

    let crit = c
        .query(
            "select c.node_id, c.id, c.ord, c.text, c.taken_by, c.done_at,
                    (select count(*) from items t where t.criterion_id = c.id)
               from criteria c order by c.node_id, c.ord",
            &[],
        )
        .await?;

    let placed = c
        .query(
            "select t.node_id, t.id, t.said,
                    (select count(*) from needs n
                      left join answers a on a.item_id = t.id and a.field = n.field
                     where n.kind = t.kind and n.required and a.field is null),
                    k.id, k.said, k.author, to_char(k.said_at,'DD.MM'),
                    (select string_agg(a.field || ': ' || a.value, ' · ' order by a.field)
                       from answers a where a.item_id = t.id),
                    t.criterion_id
               from items t join intake k on k.id = t.intake_id
              where t.node_id is not null order by t.id",
            &[],
        )
        .await?;

    let mut kids: HashMap<String, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (i, r) in nodes.iter().enumerate() {
        match r.get::<_, Option<String>>(2) {
            Some(p) => kids.entry(p).or_default().push(i),
            None => roots.push(i),
        }
    }

    // Наверху считаем ТРЕБОВАНИЯ: сколько можно взять и сколько ещё не разобрано.
    // Пункты intake тут ни при чём — их видно в своём месте.
    let mut open_items = 0usize;   // можно взять: условия выписаны
    let mut unsorted = 0usize;     // ждут разбора: условий нет
    let mut closed_items = 0usize; // выполнено
    for r in &nodes {
        if r.get::<_, String>(1) != "requirement" {
            continue;
        }
        let nid: String = r.get(0);
        let n = crit.iter().filter(|x| x.get::<_, String>(0) == nid).count();
        let d = crit.iter().filter(|x| x.get::<_, String>(0) == nid
            && x.get::<_, Option<std::time::SystemTime>>(5).is_some()).count();
        if n == 0 { unsorted += 1 } else if d == n { closed_items += 1 } else { open_items += 1 }
    }

    let mut tree = String::new();
    for i in &roots {
        draw(&mut tree, &nodes, &kids, &placed, &crit, *i);
    }
    if nodes.is_empty() {
        tree.push_str("<p class=\"none\">Дерево пустое.</p>");
    }

    // Справа — ОЧЕРЕДЬ, а не копия дерева. Пункт, который уже лёг в дерево,
    // виден там и здесь не повторяется: иначе доска показывает одно и то же дважды,
    // и непонятно, где смотреть.
    let mut queue = String::new();
    let mut archive = String::new();
    let mut n_queue = 0usize;
    let mut n_arch = 0usize;
    for it in &intakes {
        let iid: String = it.get(0);
        let said: String = it.get(1);
        let author: String = it.get(2);
        let at: String = it.get(3);

        let mine_all: Vec<_> = items.iter().filter(|r| r.get::<_, String>(1) == iid).collect();
        // Ждёт разбора: род не назначен, либо назначен, но в дерево не положен.
        let pending = mine_all.iter().any(|r| {
            let placed: Option<String> = r.get(7);
            let outcome: Option<String> = r.get(4);
            placed.is_none() && outcome.as_deref() != Some("refused")
                && outcome.as_deref() != Some("elsewhere")
        }) || mine_all.is_empty();
        let refused = mine_all.iter().any(|r| {
            matches!(r.get::<_, Option<String>>(4).as_deref(), Some("refused") | Some("elsewhere"))
        });
        // Уложенное целиком внизу не показывается вовсе: оно видно на своих задачах,
        // а список-дубль отвечал бы на вопрос, которого никто не задавал.
        if !pending && !refused {
            continue;
        }
        if pending { n_queue += 1 } else { n_arch += 1 }
        let body = if pending { &mut queue } else { &mut archive };

        body.push_str(&format!(
            "<article class=\"intake\">\n<header>\n<div class=\"meta\"><span class=\"id\">{}</span>\
             <span>{}</span><span>{}</span></div>\n<blockquote>{}</blockquote>\n</header>\n",
            esc(&iid), esc(&author), esc(&at), esc(&said)
        ));

        let mine = mine_all;
        if mine.is_empty() {
            body.push_str("<p class=\"none\">На пункты не разобран</p>\n");
        } else {
            body.push_str("<ul class=\"items\">\n");
            for r in mine {
                let id: String = r.get(0);
                let said: String = r.get(2);
                let kind: Option<String> = r.get(3);
                let outcome: Option<String> = r.get(4);
                let why: Option<String> = r.get(5);
                let by: Option<String> = r.get(6);

                let asks: Vec<(String, String)> = waiting
                    .iter()
                    .filter(|w| w.get::<_, String>(0) == id)
                    .map(|w| (w.get::<_, String>(1), w.get::<_, String>(2)))
                    .collect();
                let got: Vec<(String, String, String)> = answers
                    .iter()
                    .filter(|a| a.get::<_, String>(0) == id)
                    .map(|a| (a.get::<_, String>(1), a.get::<_, String>(2), a.get::<_, String>(3)))
                    .collect();

                let in_tree: Option<String> = r.get(7);
                let (cls, chip) = match (outcome.as_deref(), in_tree.as_deref(), kind.as_deref(), asks.is_empty()) {
                    (Some("refused"), _, _, _) => ("shut", "не делаем".to_string()),
                    (Some("elsewhere"), _, _, _) => ("shut", "решается иначе".to_string()),
                    (_, Some(n), _, _) => ("done", format!("в дереве · {n}")),
                    (_, None, None, _) => ("cold", "ждёт разбора".to_string()),
                    (_, None, Some(k), false) => ("wait", format!("{k} · ждёт ответов")),
                    (_, None, Some(k), true) => ("go", format!("{k} · готов к укладке")),
                };

                body.push_str(&format!(
                    "<li class=\"item {cls}\">\n<div class=\"line\"><span class=\"iid\">{}</span>\
                     <span class=\"chip\">{}</span></div>\n<p class=\"what\">{}</p>\n",
                    esc(&id), esc(&chip), esc(&said)
                ));

                if let (Some(w), Some(b)) = (&why, &by) {
                    body.push_str(&format!(
                        "<p class=\"why\">{} <span class=\"by\">— {}</span></p>\n",
                        esc(w), esc(b)
                    ));
                }
                if !asks.is_empty() {
                    body.push_str("<div class=\"asks\"><span class=\"lbl\">ждёт ответа</span><ul>\n");
                    for (f, a) in &asks {
                        body.push_str(&format!(
                            "<li><code>{}</code> {}</li>\n", esc(f), esc(a)
                        ));
                    }
                    body.push_str("</ul></div>\n");
                }
                if !got.is_empty() {
                    body.push_str("<div class=\"got\">\n");
                    for (f, v, by) in &got {
                        body.push_str(&format!(
                            "<p><code>{}</code> {} <span class=\"by\">— {}</span></p>\n",
                            esc(f), esc(v), esc(by)
                        ));
                    }
                    body.push_str("</div>\n");
                }
                body.push_str("</li>\n");
            }
            body.push_str("</ul>\n");
        }
        body.push_str("</article>\n");
    }

    // Внизу — только то, чему в дереве места нет: ещё не уложенное и закрытое.
    // Всё уложенное светится на своей задаче и здесь не повторяется.
    let mut tail = String::new();
    if n_queue > 0 {
        tail.push_str(&format!(
            "<section class=\"tail\"><h2>Не разобрано · {n_queue}</h2>{queue}</section>"
        ));
    }
    if n_arch > 0 {
        tail.push_str(&format!(
            "<section class=\"tail\"><details class=\"arch\"><summary><span class=\"lvl\">решили не делать</span>\
             <span class=\"ttl\">{n_arch}</span></summary><div class=\"inner\">{archive}</div></details></section>"
        ));
    }
    let body = format!("<div class=\"one\">{tree}{tail}</div>\n");

    Ok((body, intakes.len(), open_items, unsorted, closed_items))
}

/// Свод по поддереву: сколько пунктов собрано, сколько ждёт ответов и сколько
/// требований пока пустые. Это КАРТИНА, а не приговор: у домена своего «готово»
/// нет, потому что «Рост готов» — фраза без смысла, пока внутри и зелёное, и
/// красное. Но видеть, где дыра, надо не разворачивая всё дерево.
fn tally_of(
    nodes: &[tokio_postgres::Row],
    kids: &HashMap<String, Vec<usize>>,
    placed: &[tokio_postgres::Row],
    crit: &[tokio_postgres::Row],
    i: usize,
) -> (usize, usize, usize) {
    let id: String = nodes[i].get(0);
    let kind: String = nodes[i].get(1);
    let mut go = 0;
    let mut wait = 0;
    let mut empty = 0;

    // Считаем ТРЕБОВАНИЯ, а не задачи: единица работы теперь требование,
    // и свод должен говорить о том же, что и строки под ним.
    if kind == "requirement" {
        let n = crit.iter().filter(|x| x.get::<_, String>(0) == id).count();
        let d = crit.iter().filter(|x| x.get::<_, String>(0) == id
            && x.get::<_, Option<std::time::SystemTime>>(5).is_some()).count();
        if n == 0 { empty += 1 } else if d == n { go += 1 } else { wait += 1 }
    }
    for k in kids.get(&id).map(Vec::as_slice).unwrap_or(&[]) {
        let (a, b, c) = tally_of(nodes, kids, placed, crit, *k);
        go += a;
        wait += b;
        empty += c;
    }
    (go, wait, empty)
}

/// На заголовке — ровно одно число: сколько требований можно взять прямо сейчас.
/// «Сколько готово» не показываем: эта цифра ни на что не влияет и только
/// отвлекает от единственного вопроса, который задают, глядя на доску.
fn bar(_go: usize, wait: usize, empty: usize) -> String {
    if wait == 0 && empty == 0 {
        return String::new();
    }
    let mut out = String::from("<span class=\"cnt\">");
    if wait > 0 {
        out.push_str(&format!("<button class=\"take\" data-f=\"s-go\">{wait} можно взять</button>"));
    }
    if empty > 0 {
        out.push_str(&format!("<button class=\"hold\" data-f=\"s-raw\">{empty} ждут разбора</button>"));
    }
    out.push_str("</span>");
    out
}

/// Обход сверху вниз. Домен и капабилити сворачиваются, требование — нет:
/// оно и есть то, ради чего открывали.
fn draw(
    out: &mut String,
    nodes: &[tokio_postgres::Row],
    kids: &HashMap<String, Vec<usize>>,
    placed: &[tokio_postgres::Row],
    crit: &[tokio_postgres::Row],
    i: usize,
) {
    let id: String = nodes[i].get(0);
    let kind: String = nodes[i].get(1);
    let title: String = nodes[i].get(3);
    let (go, wait, empty) = tally_of(nodes, kids, placed, crit, i);
    let children = kids.get(&id).map(Vec::as_slice).unwrap_or(&[]);

    match kind.as_str() {
        "requirement" => {
            let mine: Vec<_> = placed.iter().filter(|p| p.get::<_, String>(0) == id).collect();
            let body: Option<String> = nodes[i].get(4);
            let source: Option<String> = nodes[i].get(5);
            let taken: Option<String> = nodes[i].get(6);
            let done: Option<std::time::SystemTime> = nodes[i].get(7);
            let cls = if source.is_some() { "req cited" } else { "req bare" };

            // Требование — единица работы. Готово к работе то, что сформулировано
            // и подкреплено: тело сказано, источник назван. Без них брать нечего —
            // исполнитель всё равно придёт переспрашивать.
            let mycrit: Vec<_> = crit.iter().filter(|x| x.get::<_, String>(0) == id).collect();
            let _unused_done = 0; let c_done = mycrit.iter().filter(|x| x.get::<_, Option<std::time::SystemTime>>(5).is_some()).count();
            let ready = !mycrit.is_empty();
            let c_taken = mycrit.iter().filter(|x| x.get::<_, Option<String>>(4).is_some()).count();
            let n = mycrit.len();
            // Состояние показывает ХОД, а не только крайние точки: «можно брать»
            // на половине выполненных условий скрывало бы всю проделанную работу.
            let (state_cls, state) = if n == 0 {
                ("s-raw", "ждёт разбора".to_string())
            } else if c_done == n {
                ("s-done", format!("готово {n} из {n}"))
            } else if c_done > 0 {
                ("s-run", format!("готово {c_done} из {n}"))
            } else if c_taken > 0 {
                ("s-run", format!("готово 0 из {n}"))
            } else {
                // Ни один AC не начат — значит требование свободно.
                // Форма та же, что у остальных: число всегда про ГОТОВЫЕ.
                ("s-go", format!("готово 0 из {n}"))
            };
            let _ = (done, taken, ready);

            out.push_str(&format!(
                "<details class=\"{cls}\" data-st=\"{state_cls}\"><summary>\
<span class=\"rhead\"><span class=\"lvl\">requirement</span>\
                 <span class=\"rno\">{}</span>\
                 <button class=\"rid\" data-id=\"{}\" data-copy=\"{}\" title=\"скопировать\" aria-label=\"скопировать\">\
                 <svg viewBox=\"0 0 16 16\" width=\"11\" height=\"11\" aria-hidden=\"true\">\
                 <rect x=\"5.4\" y=\"1.6\" width=\"9\" height=\"9\" rx=\"1.6\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\"/>\
                 <path d=\"M10.6 13.2v.8a1.6 1.6 0 0 1-1.6 1.6H3a1.6 1.6 0 0 1-1.6-1.6V6.4A1.6 1.6 0 0 1 3 4.8h.8\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\"/>\
                 </svg></button></span>\
                 <span class=\"req-t\">{}</span>{}\
                 <span class=\"st {state_cls}\">{}</span></summary><div class=\"req-in\">",
                esc(&id), esc(&id), esc(&format!("{id} — {title}")),
                esc(&title),
                match &source {
                    Some(src) => format!("<span class=\"cite\">{}</span>", esc(src)),
                    None => "<span class=\"cite none-cite\">источник не указан</span>".to_string(),
                },
                esc(&state)
            ));
            if let Some(b) = &body {
                out.push_str(&format!("<p class=\"req-b\">{}</p>", esc(b)));
            }
            // AC — то, что берут в работу. Требование целиком не берут:
            // «порог 10 000» и «курс хранится рядом с транзакцией» делают
            // разные люди в разное время, но проверяются они вместе.
            if mycrit.is_empty() {
                out.push_str("<p class=\"none\">AC не выписаны — взять в работу нечего</p>");
            } else {
                out.push_str("<ul class=\"crit\">");
                for x in &mycrit {
                    let t: String = x.get(3);
                    let who: Option<String> = x.get(4);
                    let fin: Option<std::time::SystemTime> = x.get(5);
                    let (cc, mark) = if fin.is_some() { ("c-done", "✓") }
                        else if who.is_some() { ("c-run", "•") } else { ("c-open", "") };
                    let cid: i64 = x.get(1);
                    let nwork: i64 = x.get(6);
                    let work: Vec<_> = placed.iter()
                        .filter(|p| p.get::<_, Option<i64>>(9) == Some(cid)).collect();
                    let tail = if work.is_empty() {
                        String::new()
                    } else {
                        // Работа агентов: только смотреть. Оператор правит AC,
                        // а не то, что агенты под них наделали.
                        let mut w = format!(
                            "<details class=\"work\"><summary>работа · {nwork}</summary><ul>"
                        );
                        for p in &work {
                            w.push_str(&format!(
                                "<li><span class=\"iid\">{}</span>{}</li>",
                                esc(&p.get::<_, String>(4)), esc(&p.get::<_, String>(2))
                            ));
                        }
                        w.push_str("</ul></details>");
                        w
                    };
                    out.push_str(&format!(
                        "<li class=\"{cc}\"><div class=\"crow\"><span class=\"mk\">{}</span>\
                         <span class=\"cx\">{}</span>{}</div>{}</li>",
                        mark, esc(&t),
                        match &who { Some(w) => format!("<span class=\"who\">{}</span>", esc(w)), None => String::new() },
                        tail
                    ));
                }
                out.push_str("</ul>");
            }
            // Происхождение: из чего это выросло. Свёрнуто — оно нужно при споре
            // о формулировке, а не при каждом взгляде на доску.
            if !mine.is_empty() {
                out.push_str(&format!(
                    "<details class=\"src\"><summary>откуда · {}</summary><ul class=\"orig\">",
                    mine.len()
                ));
                for p in &mine {
                    let src_id: String = p.get(4);
                    let src_said: String = p.get(5);
                    let src_by: String = p.get(6);
                    out.push_str(&format!(
                        "<li><span class=\"iid\">{} · {}</span>{}</li>",
                        esc(&src_id), esc(&src_by), esc(&src_said)
                    ));
                }
                out.push_str("</ul></details>");
            }
            out.push_str("</div></details>");
        }
        _ => {
            let (tag, lvl) = if kind == "domain" { ("dom", "domain") } else { ("cap", "capability") };
            // Всё закрыто при загрузке: доска открывается свёрнутой, и человек
            // сам решает, куда лезть. Раскрывает только фильтр — и ровно то,
            // что нашёл.
            let open = "";
            // Термин до двоеточия — то, как это зовут вслух; он цепляется глазом первым.
            let shown = match title.split_once(": ") {
                Some((term, rest)) => format!("<b class=\"term\">{}</b> {}", esc(term), esc(rest)),
                None => esc(&title),
            };
            out.push_str(&format!(
                "<details class=\"{tag}\"{open} data-node=\"{}\"><summary>\
                 <span class=\"head\"><span class=\"lvl\">{lvl}</span>\
                 <span class=\"ttl\">{}</span></span>{}\
                 </summary><div class=\"inner\">",
                esc(&id), shown, bar(go, wait, empty)
            ));
            if children.is_empty() {
                out.push_str("<p class=\"none\">пусто</p>");
            }
            for k in children {
                draw(out, nodes, kids, placed, crit, *k);
            }
            out.push_str("</div></details>");
        }
    }
}

/// Уровни называются по-английски — как мы условились говорить о них вслух.
fn lvl_name(kind: &str) -> &'static str {
    match kind {
        "domain" => "domain",
        "capability" => "capability",
        "requirement" => "requirement",
        _ => "",
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn tally(_n_intake: usize, open: usize, cold: usize, _shut: usize) -> String {
    format!(
        r#"<div class="tally">
  <button class="big" data-f="s-go"><b>{open}</b> можно взять в работу</button>
  <button class="big" data-f="s-raw"><b class="dim">{cold}</b> ждут разбора</button>
  <button class="clr" data-f="">показать всё</button>
</div>"#
    )
}

fn page(body: &str, n_intake: usize, open: usize, cold: usize, shut: usize, live: bool) -> String {
    let _ = n_intake;
    format!(
r#"<!doctype html><html lang="ru"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>joppa</title>
<style>
:root{{
 /* Одна гарнитура. Иерархия — весом, кеглем и цветом, а не сменой шрифта:
    смешение гарнитур размывает иерархию, а не создаёт её. */
 --ui:-apple-system,BlinkMacSystemFont,"SF Pro Text","Segoe UI",Roboto,
      "Helvetica Neue",Arial,sans-serif;
 /* Моноширинная только там, где цифры и коды стоят в колонку. */
 --mono:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,monospace;
 /* Шкала кеглей: семь ступеней, между ними не встаём. */
 --t1:.69rem; --t2:.75rem; --t3:.81rem; --t4:.88rem;
 --t5:.94rem; --t6:1.06rem; --t7:1.31rem;
 --ind:.95rem;--paper:#F2F3F1;--raised:#FBFBFA;--ink:#16191A;--muted:#5E6766;
 --rule:#D5D9D6;--rule-soft:#E4E7E5;--accent:#1F5F5B;--accent-soft:#E2ECEA;
 --stop:#9C2B20;--stop-soft:#F5E6E3;--go:#3F6B33;--go-soft:#E7EEE3;
 --raw:#8A6A1F;--raw-soft:#F3EDDD}}
@media(prefers-color-scheme:dark){{:root{{--paper:#121414;--raised:#1A1D1D;--ink:#E6E9E7;
 --muted:#9AA4A2;--rule:#2C3131;--rule-soft:#232726;--accent:#6FB5AE;--accent-soft:#17302E;
 --stop:#D97A6C;--stop-soft:#2E1E1B;--go:#8FBA7C;--go-soft:#1C2618;
 --raw:#C9A85C;--raw-soft:#2A2415}}}}
*{{box-sizing:border-box}}
body{{margin:0;background:var(--paper);color:var(--ink);
 font-family:var(--ui);font-size:16px;line-height:1.5;
 font-variant-numeric:tabular-nums;
 -webkit-font-smoothing:antialiased;text-rendering:optimizeLegibility}}
.wrap{{max-width:62rem;margin:0 auto;padding:2.6rem 1.6rem 5rem}}
h1{{font-weight:600;font-size:1.75rem;letter-spacing:-.022em;margin:0 0 .5rem}}
.tally{{display:flex;flex-wrap:wrap;gap:.3rem 1.6rem;align-items:baseline;
 font-size:var(--t4);color:var(--muted);padding-bottom:1.2rem;
 border-bottom:1px solid var(--rule);margin-bottom:2rem}}
.tally b{{color:var(--go);font-weight:600;font-size:var(--t6);margin-right:.2rem}}
.tally b.dim{{color:var(--raw)}}
.tally button{{font:inherit;color:inherit;background:none;border:none;padding:.2rem .1rem;
 cursor:pointer;border-bottom:2px solid transparent}}
.tally button:hover{{border-bottom-color:var(--rule)}}
.tally button.on{{border-bottom-color:var(--accent)}}
.clr{{font-size:var(--t3);opacity:.6}}
body.filtering .req{{display:none}}
body.filtering .req.hit{{display:block}}
body.filtering .dom,body.filtering .cap{{display:none}}
body.filtering .dom.hit,body.filtering .cap.hit{{display:block}}
body.filtering .cnt{{display:none}}

/* фильтр по месту: прячет только внутри той карточки, где нажали */
details[data-pick] .req{{display:none}}
details[data-pick="s-go"] .req[data-st="s-go"],
details[data-pick="s-raw"] .req[data-st="s-raw"],
details[data-pick="s-run"] .req[data-st="s-run"]{{display:block}}
details[data-pick] .cap{{display:none}}
details[data-pick] .cap.keep{{display:block}}
.cnt button.on{{outline:2px solid currentColor;outline-offset:1px}}
.intake{{margin-bottom:2.6rem;padding-bottom:2rem;border-bottom:1px solid var(--rule-soft)}}
.intake:last-child{{border-bottom:none}}
.meta{{display:flex;flex-wrap:wrap;gap:.3rem 1.1rem;font-family:var(--mono);
 font-size:var(--t1);color:var(--muted);margin-bottom:.6rem}}
.meta .id{{color:var(--accent);font-weight:500}}
blockquote{{margin:0 0 1.2rem;font-family:var(--ui);font-size:var(--t4);
 line-height:1.5;padding-left:.9rem;border-left:2px solid var(--rule)}}
ul.items{{list-style:none;margin:0;padding:0;display:flex;flex-direction:column;gap:.7rem}}
.item{{border:1px solid var(--rule);border-radius:3px;padding:.75rem .9rem;background:var(--raised);
 border-left-width:2px}}
.item.go{{border-left-color:var(--go)}}
.item.wait{{border-left-color:var(--raw)}}
.item.shut{{border-left-color:var(--stop);opacity:.72}}
.item.cold{{border-left-color:var(--rule)}}
.line{{display:flex;gap:.7rem;align-items:baseline;justify-content:space-between;margin-bottom:.3rem}}
.iid{{font-family:var(--mono);font-size:var(--t1);color:var(--muted)}}
.chip{{font-family:var(--mono);font-size:var(--t1);letter-spacing:.05em;
 padding:.13rem .45rem;border-radius:2px;white-space:nowrap}}
.go .chip{{color:var(--go);background:var(--go-soft)}}
.wait .chip{{color:var(--raw);background:var(--raw-soft)}}
.shut .chip{{color:var(--stop);background:var(--stop-soft)}}
.cold .chip{{color:var(--muted);background:var(--rule-soft)}}
.what{{margin:0;font-size:var(--t5)}}
.why{{margin:.5rem 0 0;font-size:var(--t4);color:var(--muted);padding-left:.7rem;
 border-left:2px solid var(--stop-soft)}}
.by{{font-family:var(--mono);font-size:var(--t1)}}
.asks{{margin-top:.6rem;padding:.55rem .7rem;background:var(--raw-soft);border-radius:2px}}
.asks .lbl{{font-family:var(--mono);font-size:var(--t1);letter-spacing:.09em;
 text-transform:uppercase;color:var(--raw)}}
.asks ul{{margin:.3rem 0 0;padding-left:1.1rem;font-size:var(--t4)}}
.asks li{{margin-bottom:.15rem}}
.got{{margin-top:.6rem;font-size:var(--t4)}}
.got p{{margin:0 0 .25rem}}
code{{font-family:var(--mono);font-size:.88em;background:var(--rule-soft);
 padding:.08em .3em;border-radius:3px}}
.none{{color:var(--muted);font-size:var(--t2);margin:.35rem 0 0;opacity:.75}}

h2{{font-size:var(--t2);font-weight:600;letter-spacing:.07em;
 text-transform:uppercase;color:var(--muted);
 margin:0 0 1.2rem;padding-bottom:.5rem;border-bottom:1px solid var(--rule)}}
.one{{max-width:60rem}}
.tail{{margin-top:2.4rem}}

/* ── дерево ── */
/* Четыре уровня различаются ПОРОДОЙ, а не вложенными рамками. Коробка только
   у домена; дальше — гарнитура, кегль, цвет и воздух. Рамка внутри рамки
   внутри рамки читается как мусор, сколько её ни скругляй. */

.dom{{margin-bottom:1.1rem;border:1px solid var(--rule);border-radius:5px;
 background:var(--raised);overflow:hidden}}
.dom>summary{{padding:.6rem 1rem;background:var(--accent-soft);
 font-size:var(--t5);font-weight:700;letter-spacing:-.006em}}
.dom[open]>summary{{border-bottom:1px solid var(--rule)}}
.dom>.inner{{padding:.2rem 1.15rem 1.1rem}}

/* капабилити — узкая моноширинная метка: ни с чем не спутать */
.cap{{margin:0;padding:1rem 0 .3rem;border-top:1px solid var(--rule)}}
.cap:first-child{{border-top:none;padding-top:.7rem}}
.cap>summary{{padding:0 0 .4rem;font-size:var(--t5);font-weight:600;
 letter-spacing:-.006em;color:var(--ink)}}
.cap>.inner{{padding:0}}
.cap .term{{color:var(--accent);font-weight:600}}

summary{{cursor:pointer;list-style:none;display:flex;flex-wrap:wrap;
 align-items:center;gap:.3rem .7rem}}
summary::-webkit-details-marker{{display:none}}
summary>*{{margin-top:0;margin-bottom:0}}
.head{{display:flex;align-items:center;gap:.5rem;flex:1;min-width:0}}
.rhead{{display:flex;align-items:center;gap:.45rem;flex:none}}
.head::before,.rhead::before{{content:"";flex:none;display:block;width:0;height:0;
 border-left:5px solid currentColor;border-top:4px solid transparent;
 border-bottom:4px solid transparent;transition:transform .15s;opacity:.35;
 transform-origin:2px 50%}}
details[open]>summary .head::before,
.req[open]>summary .rhead::before{{transform:rotate(90deg)}}
summary:hover .head::before,summary:hover .rhead::before{{opacity:.8}}
.ttl{{flex:1;min-width:0;overflow-wrap:anywhere}}
.term{{font-weight:700}}
.lvl{{font-size:var(--t1);letter-spacing:.09em;text-transform:uppercase;
 font-weight:500;flex:none;color:var(--muted);opacity:.5;line-height:1.35}}
.cap>summary .lvl{{display:none}}

.cnt{{display:flex;flex-wrap:wrap;gap:.35rem;margin:0 0 0 auto;
 font-size:var(--t1);font-weight:500}}
.take,.hold{{padding:.16rem .45rem;border-radius:4px;border:none;font:inherit;cursor:pointer}}
.take{{color:var(--go);background:var(--go-soft)}}
.hold{{color:var(--raw);background:var(--raw-soft)}}
.take:hover,.hold:hover{{outline:1px solid currentColor;outline-offset:1px}}
.cnt button.on{{outline:2px solid currentColor;outline-offset:1px}}

/* требование — без коробки: только рейка слева и вес шрифта */
.req{{margin:.7rem 0;padding-left:.8rem;border-left:2px solid}}
.req:first-child{{margin-top:.5rem}}
.req.cited{{border-left-color:var(--accent)}}
.req.bare{{border-left-color:var(--rule)}}
.req>summary{{padding:0;gap:.25rem .6rem}}
.req-t{{flex-basis:100%;margin:0;overflow-wrap:anywhere;
 font-size:var(--t5);font-weight:500;line-height:1.38;letter-spacing:-.008em}}
.req.bare .req-t{{font-weight:500;color:var(--muted)}}
.cite{{flex-basis:100%;margin:0;font-size:var(--t1);font-weight:500;
 color:var(--accent)}}
.cite.none-cite{{color:var(--raw)}}
.req-in{{padding:.35rem 0 0}}
.req-b{{margin:0 0 .5rem;font-size:var(--t3);line-height:1.55;color:var(--muted);max-width:42rem}}

.rno{{font-family:var(--mono);font-size:var(--t1);color:var(--muted);line-height:1.35}}
.rid{{display:inline-flex;align-items:center;justify-content:center;
 width:1.1rem;height:1.1rem;padding:0;border:none;background:none;cursor:pointer;
 color:var(--muted);opacity:.4;border-radius:2px}}
.rid:hover{{opacity:1;color:var(--accent);background:var(--accent-soft)}}
.rid.copied{{opacity:1;color:var(--go);background:var(--go-soft);
 animation:bump .55s cubic-bezier(.34,1.5,.5,1)}}
.rid.failed{{opacity:1;color:var(--stop);background:var(--stop-soft);animation:shake .3s ease}}
.rid .tick{{stroke-dasharray:18;stroke-dashoffset:18;animation:draw .28s .04s ease forwards}}
@keyframes bump{{0%{{transform:scale(1)}}35%{{transform:scale(1.2)}}70%{{transform:scale(1.2)}}100%{{transform:scale(1)}}}}
@keyframes draw{{to{{stroke-dashoffset:0}}}}
@keyframes shake{{0%,100%{{transform:translateX(0)}}25%{{transform:translateX(-2px)}}75%{{transform:translateX(2px)}}}}

.st{{flex:none;margin:0 0 0 auto;white-space:nowrap;font-size:var(--t1);
 font-weight:500;padding:.16rem .45rem;border-radius:4px}}
.s-raw{{color:var(--raw);background:var(--raw-soft)}}
.s-go{{color:var(--go);background:var(--go-soft)}}
.s-run{{color:var(--accent);background:var(--accent-soft)}}
.s-done{{color:var(--go);background:transparent;border:1px solid var(--go-soft)}}

/* AC — то, что берут в работу */
ul.crit{{list-style:none;margin:0;padding:0;display:flex;flex-direction:column}}
ul.crit li{{font-size:var(--t4);line-height:1.5;padding:.38rem 0;
 border-top:1px solid var(--rule-soft);overflow-wrap:anywhere}}
ul.crit li:first-child{{border-top:none;padding-top:.1rem}}
.crow{{display:flex;gap:.45rem;align-items:baseline}}
.mk{{flex:none;width:1rem;font-size:var(--t3);color:var(--go);text-align:center}}
.c-open .mk::before{{content:"☐";opacity:.35;font-size:var(--t6)}}
.c-run .mk{{color:var(--accent);font-size:var(--t6);line-height:1}}
.c-done .cx{{color:var(--muted)}}
.c-done .mk{{color:var(--go)}}
.cx{{flex:1;min-width:0;overflow-wrap:anywhere}}
.who{{margin-left:auto;flex:none;white-space:nowrap;font-size:var(--t1);
 font-weight:500;color:var(--accent)}}

details.work{{margin:.25rem 0 .1rem 1.35rem}}
details.work>summary{{font-size:var(--t1);font-weight:500;color:var(--muted);
 opacity:.65;gap:.35rem}}
details.work>summary:hover{{opacity:1}}
details.work>summary::before{{content:"";flex:none;display:block;width:0;height:0;
 border-left:4px solid currentColor;border-top:3px solid transparent;
 border-bottom:3px solid transparent;opacity:.6}}
details.work[open]>summary::before{{transform:rotate(90deg)}}
details.work ul{{list-style:none;margin:.25rem 0 0;padding:0 0 0 .7rem;
 border-left:1px solid var(--rule-soft);display:flex;flex-direction:column;gap:.25rem}}
details.work li{{font-size:var(--t2);line-height:1.45;color:var(--muted);border:none;padding:0}}
details.work .iid{{display:block;font-family:var(--mono);font-size:var(--t1);opacity:.6}}

details.src{{margin-top:.55rem;padding-top:.45rem;border-top:1px solid var(--rule-soft)}}
details.src>summary{{font-size:var(--t1);font-weight:500;color:var(--muted);
 opacity:.6;gap:.4rem}}
details.src>summary:hover{{opacity:1}}
details.src>summary::before{{content:"";flex:none;display:block;width:0;height:0;
 border-left:4px solid currentColor;border-top:3px solid transparent;
 border-bottom:3px solid transparent}}
details.src[open]>summary::before{{transform:rotate(90deg)}}
ul.orig{{list-style:none;margin:.4rem 0 0;padding:0 0 0 .8rem;
 display:flex;flex-direction:column;gap:.4rem;border-left:1px solid var(--rule-soft)}}
ul.orig li{{font-size:var(--t2);line-height:1.5;color:var(--muted)}}
ul.orig .iid{{display:block;font-family:var(--mono);font-size:var(--t1);
 opacity:.6;margin-bottom:.1rem}}

/* ── intake ── */
.intake{{margin-bottom:1.5rem;border:1px solid var(--rule);border-radius:4px;
 background:var(--raised);overflow:hidden;padding-bottom:0}}
.intake header{{padding:.75rem .9rem .8rem;border-bottom:1px solid var(--rule-soft)}}
.meta{{display:flex;flex-wrap:wrap;gap:.3rem .9rem;font-family:var(--mono);
 font-size:var(--t1);color:var(--muted);margin-bottom:.5rem}}
.meta .id{{color:var(--accent);font-weight:500}}
blockquote{{margin:0;font-size:var(--t4);line-height:1.45;padding:0;border:none}}
ul.items{{list-style:none;margin:0;padding:.5rem .9rem .9rem;
 display:flex;flex-direction:column;gap:.5rem}}
.item{{border-radius:3px;padding:.55rem .7rem;border-left:2px solid;
 background:var(--paper)}}
.item.go{{border-left-color:var(--go)}}
.item.wait{{border-left-color:var(--raw)}}
.item.shut{{border-left-color:var(--stop);opacity:.6}}
.item.cold{{border-left-color:var(--rule)}}
.line{{display:flex;gap:.6rem;align-items:baseline;justify-content:space-between;
 margin-bottom:.25rem}}
.iid{{font-family:var(--mono);font-size:var(--t1);color:var(--muted);flex:none}}
.chip{{font-size:var(--t1);font-weight:500;padding:.12rem .4rem;
 border-radius:4px;white-space:nowrap}}
.go .chip{{color:var(--go);background:var(--go-soft)}}
.wait .chip{{color:var(--raw);background:var(--raw-soft)}}
.shut .chip{{color:var(--stop);background:var(--stop-soft)}}
.cold .chip{{color:var(--muted);background:var(--rule-soft)}}
.what{{margin:0;font-size:var(--t4);line-height:1.45;overflow-wrap:anywhere}}
.why{{margin:.4rem 0 0;font-size:var(--t2);color:var(--muted);padding-left:.6rem;
 border-left:2px solid var(--stop-soft);line-height:1.45}}
.by{{font-family:var(--mono);font-size:var(--t1)}}
.asks{{margin-top:.5rem;padding:.45rem .6rem;background:var(--raw-soft);border-radius:2px}}
.asks .lbl{{font-size:var(--t1);font-weight:600;letter-spacing:.06em;
 text-transform:uppercase;color:var(--raw)}}
.asks ul{{margin:.25rem 0 0;padding-left:.95rem;font-size:var(--t3);line-height:1.45}}
.asks li{{margin-bottom:.1rem}}
.got{{margin-top:.5rem;font-size:var(--t3);line-height:1.45}}
.got p{{margin:0 0 .2rem}}
code{{font-family:var(--mono);font-size:.88em;background:var(--rule-soft);
 padding:.08em .3em;border-radius:3px}}
.live{{display:inline-block;width:.45rem;height:.45rem;border-radius:50%;margin-left:.6rem;
 vertical-align:middle;background:var(--rule)}}
.live.on{{background:var(--go)}}
.live.off{{background:var(--stop)}}

@media(max-width:640px){{
 :root{{--ind:.8rem}}
 body{{font-size:15px}}
 summary{{flex-direction:column;align-items:flex-start;gap:.28rem}}
 .head,.rhead{{width:100%}}
 .cnt,.st{{margin:0 0 0 var(--ind)}}
 .wrap{{padding:1.4rem .8rem 3rem}}
 h1{{font-size:1.45rem}}
 .tally{{gap:.5rem 1rem;font-size:var(--t2);padding-bottom:.9rem;margin-bottom:1.3rem}}
 .tally b{{font-size:var(--t6)}}
 .dom{{border-radius:4px;margin-bottom:1rem}}
 .dom>summary{{padding:.55rem .7rem}}
 .dom>.inner{{padding:.2rem .75rem .85rem}}
 .cap{{padding-top:.85rem}}
 .req{{padding-left:.65rem}}
 .req-in{{padding:.3rem 0 0}}
 .req-t{{font-size:var(--t5)}}
 .req-b{{font-size:var(--t2)}}
 ul.crit li{{font-size:var(--t3)}}
 ul.orig{{padding-left:.6rem}}
 .cap>summary{{font-size:var(--t4)}}
 .st,.cnt{{font-size:var(--t1)}}
}}
</style></head><body><div class="wrap">
<h1>joppa{live_dot}</h1>
<div id="board">{tally}{body}</div>
</div>
<script>
// Тянем только внутренность и подменяем её: перезагрузка страницы теряла бы
// прокрутку, а доску смотрят сверху вниз во время разбора.
let last = null;
let filter = '';

function applyFilter() {{
  document.body.classList.toggle('filtering', !!filter);
  document.querySelectorAll('.hit').forEach(e => e.classList.remove('hit'));
  document.querySelectorAll('.tally button').forEach(b =>
    b.classList.toggle('on', b.dataset.f === filter && filter !== ''));
  if (!filter) return;
  document.querySelectorAll('.req[data-st="' + filter + '"]').forEach(r => {{
    r.classList.add('hit');
    r.open = true;
    // раскрываем всех родителей, иначе найденное останется свёрнутым
    let p = r.parentElement;
    while (p) {{
      if (p.classList && (p.classList.contains('cap') || p.classList.contains('dom'))) {{
        p.classList.add('hit'); p.open = true;
      }}
      p = p.parentElement;
    }}
  }});
}}

// clipboard API живёт только на защищённом соединении, а доску открывают по
// обычному http внутри сети. Поэтому запасной путь через скрытое поле — иначе
// кнопка молча ничего не делает, и это худший из возможных ответов.
function copyText(text) {{
  if (navigator.clipboard && window.isSecureContext) {{
    return navigator.clipboard.writeText(text).then(() => true, () => false);
  }}
  return new Promise(res => {{
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.cssText = 'position:fixed;top:0;left:0;opacity:0;pointer-events:none';
    document.body.appendChild(ta);
    ta.select();
    ta.setSelectionRange(0, text.length);
    let ok = false;
    try {{ ok = document.execCommand('copy'); }} catch (e) {{ ok = false; }}
    document.body.removeChild(ta);
    res(ok);
  }});
}}

function flash(btn, ok) {{
  const was = btn.innerHTML;
  btn.classList.add(ok ? 'copied' : 'failed');
  btn.innerHTML = ok
    ? '<svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true">'
      + '<path class="tick" d="M2.5 8.5l3.5 3.5 7.5-8" fill="none" stroke="currentColor"'
      + ' stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>'
    : '<svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true">'
      + '<path d="M4 4l8 8M12 4l-8 8" fill="none" stroke="currentColor" stroke-width="2"'
      + ' stroke-linecap="round"/></svg>';
  setTimeout(() => {{
    btn.classList.remove('copied', 'failed');
    btn.innerHTML = was;
  }}, 1000);
}}

// Номер требования копируется в буфер: его вставляют в разговор и говорят
// «дожми это». Клик не должен сворачивать карточку, поэтому событие гасим.
// Фильтр по месту помнится по узлу: содержимое подменяется каждые две секунды,
// и без этого выбор слетал бы на первом же обновлении.
const picks = {{}};

function applyPicks() {{
  document.querySelectorAll('details[data-node]').forEach(d => {{
    const p = picks[d.dataset.node];
    if (p) {{ d.dataset.pick = p; d.open = true; }} else {{ delete d.dataset.pick; }}
    d.querySelectorAll('.cnt button').forEach(b =>
      b.classList.toggle('on', !!p && b.dataset.f === p));
  }});
  // капабилити внутри отфильтрованного домена оставляем только те, где что-то осталось
  document.querySelectorAll('details[data-pick]').forEach(d => {{
    d.querySelectorAll('.cap').forEach(c => {{
      const has = c.querySelector('.req[data-st="' + d.dataset.pick + '"]');
      c.classList.toggle('keep', !!has);
      if (has) c.open = true;
    }});
  }});
}}

document.addEventListener('click', e => {{
  const scoped = e.target.closest('.cnt [data-f]');
  if (scoped) {{
    e.preventDefault(); e.stopPropagation();
    const host = scoped.closest('details[data-node]');
    const key = host.dataset.node;
    picks[key] = (picks[key] === scoped.dataset.f) ? null : scoped.dataset.f;
    applyPicks();
    return;
  }}
  const r = e.target.closest('.rid');
  if (r) {{
    e.preventDefault(); e.stopPropagation();
    copyText(r.dataset.copy || r.dataset.id).then(ok => flash(r, ok));
    return;
  }}
  const b = e.target.closest('[data-f]');
  if (!b) return;
  e.preventDefault();
  filter = (b.dataset.f === filter) ? '' : b.dataset.f;
  applyFilter();
}});
async function tick() {{
  try {{
    const r = await fetch('body', {{cache: 'no-store'}});
    if (!r.ok) throw new Error(r.status);
    const html = await r.text();
    if (html !== last) {{ document.getElementById('board').innerHTML = html; last = html; applyFilter(); applyPicks(); }}
    document.getElementById('live').className = 'live on';
  }} catch (e) {{
    document.getElementById('live').className = 'live off';
  }}
}}
{poll}
</script>
</body></html>
"#,
        tally = tally(n_intake, open, cold, shut),
        live_dot = if live {{ r#"<span class="live" id="live"></span>"# }} else {{ "" }},
        poll = if live {{ "tick(); setInterval(tick, 2000);" }} else {{ "applyFilter(); applyPicks();" }})
}
