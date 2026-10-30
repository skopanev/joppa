//! Rendered pages: the public page, the tree, the guide and the settings.
use super::*;

/// The public origin of the pages, escaped for an HTML attribute.
fn public_origin(app: &App) -> String {
    app.oauth_origin
        .as_deref()
        .unwrap_or("https://joppa.otion.us")
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// The OpenGraph and Twitter Card tags of the page at `path`, with absolute URLs.
fn social_meta(origin: &str, path: &str) -> String {
    include_str!("social_meta.html")
        .replace("/* PAGE_URL */", &format!("{origin}{path}"))
        .replace("/* PUBLIC_ORIGIN */", origin)
}

pub(super) async fn public_page(State(app): State<App>) -> Html<String> {
    let origin = public_origin(&app);
    Html(
        include_str!("board.html")
            .replace("/* SOCIAL_META */", &social_meta(&origin, "/"))
            .replace("/* PUBLIC_ORIGIN */", &origin)
            .replace(
                "/* BOARD_STYLE */",
                concat!(include_str!("board.css"), include_str!("board_more.css")),
            )
            .replace("/* BRAND_STYLE */", include_str!("brand.css"))
            .replace("/* BOARD_IMPORTS */", &served_imports()),
    )
}

pub(super) async fn tree_page(State(app): State<App>) -> Html<String> {
    Html(
        include_str!("tree.html")
            .replace(
                "/* SOCIAL_META */",
                &social_meta(&public_origin(&app), "/tree"),
            )
            .replace(
                "/* BOARD_STYLE */",
                concat!(include_str!("board.css"), include_str!("board_more.css")),
            )
            .replace("/* BRAND_STYLE */", include_str!("brand.css"))
            .replace("/* TREE_STYLE */", include_str!("tree.css"))
            .replace("/* TREE_SCRIPT */", include_str!("tree.js")),
    )
}

pub(super) async fn guide_page(State(app): State<App>) -> Html<String> {
    Html(
        include_str!("guide.html")
            .replace(
                "/* SOCIAL_META */",
                &social_meta(&public_origin(&app), "/guide"),
            )
            .replace("/* BRAND_STYLE */", include_str!("brand.css")),
    )
}

pub(super) async fn settings_page(State(app): State<App>) -> Html<String> {
    Html(
        concat!(
            include_str!("settings.html"),
            include_str!("settings_more.html")
        )
        .replace(
            "/* SOCIAL_META */",
            &social_meta(&public_origin(&app), "/settings"),
        )
        .replace(
            "/* BOARD_STYLE */",
            concat!(include_str!("board.css"), include_str!("board_more.css")),
        )
        .replace("/* BRAND_STYLE */", include_str!("brand.css")),
    )
}
