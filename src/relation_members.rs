//! The member read of one relation (in_relation on the object list). A member of relation K
//! is a present node that holds K, also with no edges, or the present target of a K edge of a
//! present node. The read gives the members in address order, in pages of at most 25, with
//! a bare address as the cursor. It takes no other filter.
use anyhow::{Result, ensure};
use serde_json::{Value, json};

/// The refusal of a member read with any other list key.
pub const ONLY: &str = "in_relation takes only fields, limit and cursor";

/// The keys that a member read takes. The workspace and the agent alias name the caller.
pub const KEYS: [&str; 6] = [
    "workspace",
    "alias",
    "in_relation",
    "fields",
    "limit",
    "cursor",
];

/// The largest page of a member read. A larger limit gives this page.
pub const PAGE: usize = 25;

/// Refuse a member read that sends a key outside `KEYS`.
pub fn check_keys<'a>(sent: impl IntoIterator<Item = &'a str>) -> Result<()> {
    for key in sent {
        ensure!(KEYS.contains(&key), ONLY);
    }
    Ok(())
}

/// The page size of a member read: 25 without a limit, and at most 25 with one.
pub fn page(limit: Option<usize>) -> usize {
    limit.map_or(PAGE, |n| n.min(PAGE))
}

/// The relation keys that `fields` names as `relations.<key>`.
pub fn relation_keys(fields: &[String]) -> Vec<String> {
    let keys = fields.iter().filter_map(|f| f.strip_prefix("relations."));
    keys.map(|rest| rest.split('.').next().unwrap_or(rest).to_owned())
        .collect()
}

/// One item of an MCP member read with `fields`: each `relations.<path>` key moves into
/// `relations`, so `relations.swap` gives `{relations: {swap: ...}}` (ac:ig3uv35d).
pub fn nest(item: &mut Value) {
    let Some(map) = item.as_object_mut() else {
        return;
    };
    let paths: Vec<String> = map
        .keys()
        .filter(|k| k.starts_with("relations."))
        .cloned()
        .collect();
    for path in paths {
        let value = map.remove(&path).unwrap_or_default();
        let segments: Vec<&str> = path.split('.').collect();
        let relations = map.entry("relations").or_insert_with(|| json!({}));
        put(relations, &segments[1..], value);
    }
}

/// Put `value` at `segments` below `at`. A step into a value that is not an object stops.
fn put(at: &mut Value, segments: &[&str], value: Value) {
    if !(at.is_object() || at.is_null()) {
        return;
    }
    match segments {
        [last] => at[*last] = value,
        [first, rest @ ..] => put(&mut at[*first], rest, value),
        [] => {}
    }
}

#[cfg(test)]
mod nest_tests {
    use super::*;

    // ac:ig3uv35d
    #[test]
    fn a_relation_field_nests_under_relations() {
        let fields = ["title".to_owned(), "relations.swap".to_owned()];
        assert_eq!(relation_keys(&fields), ["swap"]);
        let mut item =
            json!({"address":"n:1","title":"N1","relations.swap":{"edges":[{"to":"n:2"}]}});
        nest(&mut item);
        assert_eq!(
            item,
            json!({"address":"n:1","title":"N1","relations":{"swap":{"edges":[{"to":"n:2"}]}}})
        );
        let mut whole = json!({"address":"n:3","relations":{"pay":{"edges":[]}}});
        nest(&mut whole);
        assert_eq!(
            whole,
            json!({"address":"n:3","relations":{"pay":{"edges":[]}}})
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ac:49128yzr
    #[test]
    fn a_member_read_takes_only_fields_limit_and_cursor() {
        assert!(
            check_keys([
                "workspace",
                "alias",
                "in_relation",
                "fields",
                "limit",
                "cursor"
            ])
            .is_ok()
        );
        for key in ["parent", "address", "related_to", "status", "kind", "data"] {
            let refusal = check_keys(["in_relation", key]).unwrap_err().to_string();
            assert_eq!(refusal, ONLY, "{key}");
        }
    }

    // ac:49128yzr
    #[test]
    fn a_member_read_gives_pages_of_at_most_25() {
        assert_eq!(page(None), 25);
        assert_eq!(page(Some(100)), 25);
        assert_eq!(page(Some(10)), 10);
    }
}
