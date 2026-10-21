//! Rendered pages: the snapshot, the public page and the tree page.
use super::*;

pub fn snapshot(data: &Value) -> anyhow::Result<String> {
    let mut data = data.clone();
    let state = serde_json::from_value::<joppa::model::Workspace>(data["state"].clone())?;
    {
        data["snapshot_schema"] = serde_json::json!(joppa::objects::schema(&state));
        let mut objects = serde_json::Map::new();
        for (key, object) in &state.objects {
            objects.insert(key.clone(), serde_json::json!({
                "address":key,"kind":object.kind,"id":object.id,
                "current_revision":object.current().revision,"revision_count":object.current().revision,
                "revisions":object.revisions,"signal":joppa::objects::signal(&state,key),
                "process":joppa::objects::process(&state,key),
                "work_ready":joppa::objects::work_ready(&state,key),
                "review_required":!joppa::objects::review_links(&state,key).is_empty(),
                "annotations":state.notes.get(&joppa::objects::annotation_address(&state,key)),
                "item_id":data["system_ids"][&object.kind][&object.id],
            }));
        }
        data["snapshot_objects"] = serde_json::json!(objects);
    }
    let escaped = data
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    Ok(board_page("https://joppa.otion.us", &embedded_imports())
        .replace("/* SNAPSHOT */", &format!("window.SNAPSHOT={escaped};"))
        .replace(
            "/favicon.svg",
            &format!(
                "data:image/svg+xml;base64,{}",
                base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    include_str!("favicon.svg")
                )
            ),
        ))
}

pub(super) async fn public_page(State(app): State<App>) -> Html<String> {
    Html(page_at(
        app.oauth_origin
            .as_deref()
            .unwrap_or("https://joppa.otion.us"),
    ))
}

pub(super) fn page_at(origin: &str) -> String {
    board_page(origin, &served_imports())
}

fn board_page(origin: &str, imports: &str) -> String {
    let escaped = origin
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    include_str!("board.html")
        .replace("/* PUBLIC_ORIGIN */", &escaped)
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BRAND_STYLE */", include_str!("brand.css"))
        .replace("/* BOARD_IMPORTS */", imports)
}

pub(super) fn tree_page() -> String {
    include_str!("tree.html")
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BRAND_STYLE */", include_str!("brand.css"))
        .replace("/* TREE_STYLE */", include_str!("tree.css"))
        .replace("/* TREE_SCRIPT */", include_str!("tree.js"))
}
