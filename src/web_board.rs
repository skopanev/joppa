//! The board page script. Each file of src/board is one native ES module with its own
//! imports and exports. The binary serves the files as they are, without a bundler.
use super::*;
use axum::response::{IntoResponse, Response};

/// The board modules by file name. The page imports them through an import map, so each
/// module names what it needs and the list order does not matter.
pub(super) const BOARD_MODULES: [(&str, &str); 33] = [
    ("account-menu.js", include_str!("board/account-menu.js")),
    ("api.js", include_str!("board/api.js")),
    ("board.js", include_str!("board/board.js")),
    ("card.js", include_str!("board/card.js")),
    ("clipboard.js", include_str!("board/clipboard.js")),
    ("collections.js", include_str!("board/collections.js")),
    (
        "criterion-tests.js",
        include_str!("board/criterion-tests.js"),
    ),
    ("dom.js", include_str!("board/dom.js")),
    ("feedback.js", include_str!("board/feedback.js")),
    ("filters.js", include_str!("board/filters.js")),
    ("format.js", include_str!("board/format.js")),
    ("history.js", include_str!("board/history.js")),
    ("icons.js", include_str!("board/icons.js")),
    ("links.js", include_str!("board/links.js")),
    ("main.js", include_str!("board/main.js")),
    ("markdown.js", include_str!("board/markdown.js")),
    ("motion.js", include_str!("board/motion.js")),
    ("notes.js", include_str!("board/notes.js")),
    ("object-detail.js", include_str!("board/object-detail.js")),
    ("object-list.js", include_str!("board/object-list.js")),
    ("object-page.js", include_str!("board/object-page.js")),
    ("object-types.js", include_str!("board/object-types.js")),
    ("result-filter.js", include_str!("board/result-filter.js")),
    ("search-results.js", include_str!("board/search-results.js")),
    ("revisions.js", include_str!("board/revisions.js")),
    ("sign-in.js", include_str!("board/sign-in.js")),
    ("state.js", include_str!("board/state.js")),
    ("status-hint.js", include_str!("board/status-hint.js")),
    ("status.js", include_str!("board/status.js")),
    ("theme.js", include_str!("board/theme.js")),
    ("timeline.js", include_str!("board/timeline.js")),
    ("workspaces.js", include_str!("board/workspaces.js")),
    (
        "workspace-links.js",
        include_str!("board/workspace-links.js"),
    ),
];

/// The import map of the served page: every board module loads from /board/<name>.
pub(super) fn served_imports() -> String {
    json!({"imports": {"board/": "/board/"}}).to_string()
}

/// The import map of a snapshot file. Each module is a data URL, so the file opens with no
/// server. Base64 keeps the module text free of characters that end the script element.
pub(super) fn embedded_imports() -> String {
    let imports: serde_json::Map<String, Value> = BOARD_MODULES
        .iter()
        .map(|(name, text)| {
            let data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, text);
            (
                format!("board/{name}"),
                json!(format!("data:text/javascript;base64,{data}")),
            )
        })
        .collect();
    json!({ "imports": imports }).to_string()
}

pub(super) async fn board_module(Path(name): Path<String>) -> Response {
    match BOARD_MODULES.iter().find(|(file, _)| *file == name) {
        Some((_, text)) => (
            [
                (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            *text,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    /// The server knows every file of src/board, with the same text, and no other file.
    #[test]
    fn board_modules_hold_every_file_of_the_board_directory() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/board");
        let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, std::fs::read_to_string(&path).unwrap())
            })
            .collect();
        files.sort();
        let mut served: Vec<(String, String)> = super::BOARD_MODULES
            .iter()
            .map(|(name, text)| (name.to_string(), text.to_string()))
            .collect();
        served.sort();
        assert_eq!(files, served);
    }
}
