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
/// the current status of an object comes from joppa_objects.
pub fn history(mut reply: Value) -> Value {
    remove(&mut reply, &["open", "order", "after", "limit"]);
    for item in reply
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        remove(item, &["signal"]);
    }
    reply
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

/// relations_of: no echo of the filters. An item gives its direction and kind only when the
/// request did not filter on them, and its relation always.
pub fn links(mut reply: Value, args: &Value) -> Value {
    remove(&mut reply, &["direction", "relation", "kind"]);
    let filters = ["direction", "kind"];
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
                {"address":"note:a","direction":"incoming","relation":"related","kind":"note"}]}),
            &json!({"direction":"incoming"}),
        );
        assert_eq!(
            reply,
            json!({"items":[{"address":"note:a","relation":"related","kind":"note"}]})
        );
    }
}
