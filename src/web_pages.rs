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
    Ok(page()
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

pub(super) fn page() -> String {
    page_at("https://joppa.otion.us")
}

pub(super) async fn public_page(State(app): State<App>) -> Html<String> {
    Html(page_at(
        app.oauth_origin
            .as_deref()
            .unwrap_or("https://joppa.otion.us"),
    ))
}

pub(super) fn page_at(origin: &str) -> String {
    let escaped = origin
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    include_str!("board.html")
        .replace("/* PUBLIC_ORIGIN */", &escaped)
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BRAND_STYLE */", include_str!("brand.css"))
        .replace("/* BOARD_SCRIPT */", &BOARD_SCRIPT.concat())
}

/// The board script in execution order. The parts form one script in one element, so they
/// share one scope. The file names in src/board sort in this order.
const BOARD_SCRIPT: [&str; 5] = [
    include_str!("board/10-shell.js"),
    include_str!("board/20-session.js"),
    include_str!("board/30-rows.js"),
    include_str!("board/40-pages.js"),
    include_str!("board/50-cards.js"),
];

pub(super) fn tree_page() -> String {
    include_str!("tree.html")
        .replace("/* BOARD_STYLE */", include_str!("board.css"))
        .replace("/* BRAND_STYLE */", include_str!("brand.css"))
        .replace("/* TREE_STYLE */", include_str!("tree.css"))
        .replace("/* TREE_SCRIPT */", include_str!("tree.js"))
}

#[cfg(test)]
mod tests {
    /// Every file of src/board is a part of the script, in file name order. The browser checks
    /// join the parts in the same order.
    #[test]
    fn board_script_holds_every_part_in_name_order() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/board");
        let mut names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        names.sort();
        let joined: String = names
            .iter()
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect();
        assert_eq!(joined, super::BOARD_SCRIPT.concat());
    }
}
