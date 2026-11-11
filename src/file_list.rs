//! The files of an object from its attachment map (requirement:file-upload). The HTTP object
//! read, joppa_attach and joppa_attachments give each file with a presigned download url. An
//! MCP object read signs nothing: it gives `files_count`, and `fields: ["files"]` gives each
//! file as `{id, name, size, content_type, at}`.
use anyhow::Result;
use serde_json::{Map, Value, json};

/// The keys of a short file in an MCP object read.
const SHORT: [&str; 5] = ["id", "name", "size", "content_type", "at"];

/// The attached files of `attachments` (the map of id to file), newest first, each with its
/// `id` and `created_at`. A detached file is left out. Files of one time come in id order.
pub fn present(attachments: &Value) -> Vec<Value> {
    let mut files: Vec<_> = attachments
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, file)| file["removed"] != true)
        .map(|(id, file)| {
            let mut file = file.clone();
            file["id"] = json!(id);
            file["created_at"] = file["at"].clone();
            let at = chrono::DateTime::parse_from_str(
                file["at"].as_str().unwrap_or(""),
                "%Y-%m-%d %H:%M:%S%.f%#z",
            )
            .ok();
            (at, id.clone(), file)
        })
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    files.into_iter().map(|(_, _, file)| file).collect()
}

/// Add a presigned GET of 15 minutes to each file. `folder` is the resolved workspace ID.
pub fn sign(folder: &str, files: &mut [Value]) -> Result<()> {
    for file in files {
        let text = |key: &str| file[key].as_str().unwrap_or("").to_owned();
        let key = crate::spaces::Spaces::key(folder, &text("hash"))?;
        let url =
            crate::spaces::storage()?.download_url(&key, &text("name"), &text("content_type"));
        file["url"] = json!(url);
    }
    Ok(())
}

/// One file as an MCP object read gives it: id, name, size, content type and time.
pub fn short(file: &Value) -> Value {
    let map: Map<String, Value> = SHORT
        .iter()
        .filter_map(|key| Some((key.to_string(), file.get(*key)?.clone())))
        .collect();
    Value::Object(map)
}

/// The number of attached files in `attachments`.
pub fn count(attachments: &Value) -> usize {
    let files = attachments.as_object().into_iter().flatten();
    files.filter(|(_, file)| file["removed"] != true).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_come_newest_first_without_the_detached_ones() {
        let map = json!({
            "a": {"name": "a.txt", "at": "2026-01-01 10:00:00+00", "hash": "h", "size": 1},
            "b": {"name": "b.txt", "at": "2026-01-02 10:00:00+00", "content_type": "text/plain"},
            "c": {"name": "c.txt", "at": "2026-01-03 10:00:00+00", "removed": true},
        });
        let files = present(&map);
        let ids: Vec<&str> = files.iter().filter_map(|f| f["id"].as_str()).collect();
        assert_eq!(ids, ["b", "a"]);
        assert_eq!(count(&map), 2);
        // A short file has no hash, no url and no created_at copy.
        assert_eq!(
            short(&files[1]),
            json!({"id": "a", "name": "a.txt", "size": 1, "at": "2026-01-01 10:00:00+00"})
        );
    }
}
