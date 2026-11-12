//! The MCP reply shapes (requirement:read-contract): one copy of each fact, and no echo of
//! the request arguments. The HTTP routes keep their own shapes.
use serde_json::{Value, json};

fn remove(value: &mut Value, keys: &[&str]) {
    if let Some(map) = value.as_object_mut() {
        for key in keys {
            map.remove(*key);
        }
    }
}

/// joppa_command and joppa_batch: no changed-object summaries and no public IDs. A schema
/// write gives the new schema revision, not the type map that the caller sent.
pub fn write(mut reply: Value) -> Value {
    remove(&mut reply, &["summary", "system_ids"]);
    match reply.get_mut("results").and_then(Value::as_array_mut) {
        Some(results) => results.iter_mut().for_each(operation),
        None => operation(&mut reply),
    }
    reply
}

fn operation(result: &mut Value) {
    remove(result, &["system_ids"]);
    let command = &result["command"];
    if command["op"] == "set_tree_schema"
        && let Some(revision) = command["revision"].as_u64()
    {
        result["command"] =
            json!({"op":"set_tree_schema","revision":revision,"reason":command["reason"]});
        result["schema_revision"] = json!(revision + 1);
    }
}

/// joppa_history: no echo of open, order, after and limit. An entry gives its own change;
/// the current status of an object comes from joppa_objects. An entry has no kind, no change
/// equal to its op and its times in whole seconds (ac:jkvdy2jl). Under open: true an entry
/// gives no status: each one is open.
pub fn history(mut reply: Value) -> Value {
    let open = reply["open"] == true;
    remove(&mut reply, &["open", "order", "after", "limit"]);
    for item in reply
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        remove(item, &["signal", "kind"]);
        if item["change"] == item["op"] {
            remove(item, &["change"]);
        }
        if open {
            remove(item, &["status"]);
        }
        whole_seconds(item.get_mut("at"));
        whole_seconds(item.get_mut("processed").and_then(|p| p.get_mut("at")));
    }
    reply
}

/// A time text such as `2026-11-12 10:11:12.123456+00` without its fraction of a second.
fn whole_seconds(at: Option<&mut Value>) {
    let Some(at) = at else { return };
    if let Some((head, rest)) = at.as_str().and_then(|t| t.split_once('.')) {
        let zone = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        *at = json!(format!("{head}{zone}"));
    }
}

/// joppa_search: no echo of the flags. `workspaces` shows only when the search reads all
/// workspaces of the caller.
pub fn search(mut reply: Value, all_workspaces: bool) -> Value {
    remove(&mut reply, &["all_versions", "all_workspaces"]);
    if !all_workspaces {
        remove(&mut reply, &["workspaces"]);
    }
    reply
}

/// The keys of a relations_of item that `fields` names; the address always comes.
pub const LINK_FIELDS: [&str; 7] = [
    "title",
    "kind",
    "relation",
    "direction",
    "when",
    "revision",
    "present",
];

/// Refuse a relations_of `fields` entry outside LINK_FIELDS.
pub fn link_fields(fields: &[String]) -> anyhow::Result<()> {
    if let Some(field) = fields.iter().find(|f| !LINK_FIELDS.contains(&f.as_str())) {
        anyhow::bail!(
            "unknown field {field}; relations_of takes title, kind, relation, direction, when, revision and present"
        );
    }
    Ok(())
}

/// relations_of: no echo of the filters. An item gives its direction, relation and kind only
/// when the request did not filter on them, and no `present: true` (ac:hed5l9dd). With
/// `fields` an item gives its address and the named keys.
pub fn links(mut reply: Value, args: &Value) -> Value {
    remove(&mut reply, &["direction", "relation", "kind"]);
    let filters = ["direction", "relation", "kind"];
    let fields: Option<Vec<&str>> = args["fields"]
        .as_array()
        .map(|f| f.iter().filter_map(Value::as_str).collect());
    for item in reply
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(map) = item.as_object_mut() {
            for filter in filters {
                if args.get(filter).is_some_and(Value::is_string) {
                    map.remove(filter);
                }
            }
            if map.get("present") == Some(&json!(true)) {
                map.remove("present");
            }
            if let Some(fields) = &fields {
                map.retain(|key, _| key == "address" || fields.contains(&key.as_str()));
            }
        }
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_schema_write_gives_its_revision_and_no_type_map() {
        let reply = write(
            json!({"position":9,"replayed":false,"summary":{"a":1},"system_ids":{},
            "command":{"op":"set_tree_schema","revision":19,"types":{"ac":{}},"reason":"R"}}),
        );
        assert_eq!(
            reply,
            json!({"position":9,"replayed":false,"schema_revision":20,
                "command":{"op":"set_tree_schema","revision":19,"reason":"R"}})
        );
        let batch = write(
            json!({"results":[{"position":1,"system_ids":{"x":1},"command":{"op":"note"}}],"summary":{}}),
        );
        assert_eq!(
            batch,
            json!({"results":[{"position":1,"command":{"op":"note"}}]})
        );
    }

    #[test]
    fn a_link_item_names_only_what_the_request_did_not_filter() {
        let reply = links(
            json!({"direction":"incoming","relation":null,"kind":null,"items":[
                {"address":"note:a","direction":"incoming","relation":"related","kind":"note","present":true}]}),
            &json!({"direction":"incoming"}),
        );
        assert_eq!(
            reply,
            json!({"items":[{"address":"note:a","relation":"related","kind":"note"}]})
        );
        let filtered = links(
            json!({"items":[{"address":"note:a","title":"A","direction":"incoming","relation":"related","kind":"note","present":false}]}),
            &json!({"relation":"related","kind":"note"}),
        );
        assert_eq!(
            filtered,
            json!({"items":[{"address":"note:a","title":"A","direction":"incoming","present":false}]})
        );
    }

    // ac:hed5l9dd
    #[test]
    fn a_link_item_with_fields_gives_its_address_and_the_named_keys() {
        let item = json!({"address":"ac:b","title":"B","kind":"ac","relation":"related","direction":"outgoing","present":true});
        let reply = links(json!({"items":[item]}), &json!({"fields":["title"]}));
        assert_eq!(reply, json!({"items":[{"address":"ac:b","title":"B"}]}));
        assert!(link_fields(&["title".into(), "when".into()]).is_ok());
        let refusal = link_fields(&["nope".into()]).unwrap_err().to_string();
        assert!(refusal.starts_with("unknown field nope; relations_of takes title"));
    }

    // ac:jkvdy2jl
    #[test]
    fn a_history_entry_gives_no_kind_no_repeated_change_and_whole_seconds() {
        let entries = json!([
            {"position":1,"at":"2026-11-12 10:11:12.123456+00","op":"save_object","change":"save","kind":"requirement","status":"open","signal":{}},
            {"position":2,"at":"2026-11-12 10:11:13+00","op":"archive_object","change":"archive_object","status":"processed",
                "processed":{"at":"2026-11-12 10:12:00.5+00"}}]);
        let reply = history(json!({"items":entries,"open":false,"order":"oldest"}));
        assert_eq!(
            reply,
            json!({"items":[
                {"position":1,"at":"2026-11-12 10:11:12+00","op":"save_object","change":"save","status":"open"},
                {"position":2,"at":"2026-11-12 10:11:13+00","op":"archive_object","status":"processed",
                    "processed":{"at":"2026-11-12 10:12:00+00"}}]})
        );
        let open =
            history(json!({"items":[{"position":3,"status":"open","op":"note"}],"open":true}));
        assert_eq!(open, json!({"items":[{"position":3,"op":"note"}]}));
    }
}
