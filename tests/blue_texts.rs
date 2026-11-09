//! requirement:vqlbur46: joppa_help, the guide, README.md and the docs name the tone blue for a
//! test without a result and never the tone grey. board.css has the blue dot colours. Only
//! src/status_colors*.rs (the alias and its tests) names grey in src, docs and README.md.
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

/// Each file below `dir`, at any depth.
fn files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Refuse a text that names grey, and a text that does not name blue when `blue` is true.
fn names_blue(what: &str, text: &str, blue: bool) -> Result<()> {
    let lower = text.to_lowercase();
    ensure!(!lower.contains("grey"), "{what} names grey");
    ensure!(!blue || lower.contains("blue"), "{what} does not name blue");
    Ok(())
}

// ac:i5vjohp0
// test:rl9vhy1x
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn help_and_the_guide_name_blue_and_not_grey() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let mut help = String::new();
    let sections = [
        "operations",
        "writing",
        "objects",
        "search",
        "tree",
        "examples",
    ];
    let mut args: Vec<Value> = vec![json!({})];
    args.extend(sections.iter().map(|s| json!({"section":s})));
    for a in args {
        let reply = w.mcp(&w.bot, "joppa_help", a.clone()).await?;
        let reply = reply.map_err(|e| anyhow::anyhow!("joppa_help {a}: {e}"))?;
        names_blue(&format!("joppa_help {a}"), &reply.to_string(), false)?;
        help.push_str(&reply.to_string());
    }
    names_blue("joppa_help", &help, true)?;
    let guide = w.http.get(format!("{}/guide", w.root)).send().await?;
    ensure!(
        guide.status().is_success(),
        "GET /guide: {}",
        guide.status()
    );
    names_blue("GET /guide", &guide.text().await?, true)?;
    Ok(())
}

// ac:i5vjohp0
// test:26888dl2
#[test]
fn the_readme_and_the_docs_name_blue_and_not_grey() -> Result<()> {
    let root = Path::new(ROOT);
    let mut all = read(&root.join("README.md"))?;
    names_blue("README.md", &all, false)?;
    let mut docs = Vec::new();
    files(&root.join("docs"), &mut docs)?;
    for path in docs
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
    {
        let text = read(path)?;
        names_blue(&path.display().to_string(), &text, false)?;
        all.push_str(&text);
    }
    names_blue("README.md and docs/*.md", &all, true)
}

// ac:i5vjohp0
// test:9y3pslbp
#[test]
fn board_css_sets_the_blue_dot_and_no_grey_dot() -> Result<()> {
    let css: String = read(&Path::new(ROOT).join("src/board.css"))?
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    for rule in [
        "--dot-blue:var(--dot-blue-light)",
        "--dot-blue:var(--dot-blue-dark)",
    ] {
        ensure!(css.contains(rule), "board.css has no {rule}");
    }
    ensure!(!css.contains("--dot-grey"), "board.css has --dot-grey");
    Ok(())
}

// ac:i5vjohp0
// test:y0e47yb9
#[test]
fn only_the_status_colour_alias_names_grey_in_src_docs_and_the_readme() -> Result<()> {
    let root = Path::new(ROOT);
    let mut paths = vec![root.join("README.md")];
    files(&root.join("src"), &mut paths)?;
    files(&root.join("docs"), &mut paths)?;
    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = path.strip_prefix(root)?.display().to_string();
        let alias = name.starts_with("src/status_colors") && name.ends_with(".rs");
        if text.to_lowercase().contains("grey") && !alias {
            found.push(name);
        }
    }
    ensure!(found.is_empty(), "grey outside the alias: {found:?}");
    Ok(())
}

// ac:i5vjohp0
// test:6rgulqfa
#[test]
fn the_server_readme_names_the_rollback_after_063() -> Result<()> {
    let text = read(&Path::new(ROOT).join("server/README.md"))?;
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| p.contains("063"))
        .collect();
    let right = paragraphs.iter().any(|p| {
        p.contains("previous binary")
            && p.contains("blue status list")
            && p.contains("--dot-blue")
            && p.contains("pre-deploy dump")
    });
    ensure!(right, "server/README.md after 063: {paragraphs:?}");
    Ok(())
}
