//! A live save_object of an existing object merges its data into the current data: a sent
//! field replaces its value, a null removes a stored field, and every other field stays. An omitted
//! parent keeps the current parent. `clean: true` replaces the data with the sent data. The
//! server writes the result into the command before the journal stores it, so an event
//! always names its whole data and parent.
use crate::model::Workspace;
use crate::objects::{address, resolve_address};
use anyhow::{Result, bail};
use serde_json::Value;

pub const CLEAN_REFUSAL: &str = "clean must be true or false";

/// Take the flag `clean` out of the input `value` of a save_object, before any read of it.
pub fn take_clean(value: &mut Value) -> Result<bool> {
    if value["op"] != "save_object" {
        return Ok(false);
    }
    match value.as_object_mut().and_then(|m| m.remove("clean")) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(clean)) => Ok(clean),
        Some(_) => bail!(CLEAN_REFUSAL),
    }
}

/// Complete the input `value` of one save_object from the current object. A sent value of a
/// computed field is refused, and the stored data keeps no computed field (computed_fields.rs).
pub fn merge(w: &Workspace, value: &mut Value, clean: bool) -> Result<()> {
    if value["op"] != "save_object" {
        return Ok(());
    }
    let Some(kind) = value["kind"].as_str().map(String::from) else {
        return Ok(());
    };
    crate::computed_fields::guard(w, &kind, &value["data"])?;
    merge_data(w, value, clean)?;
    if let Some(data) = value.get_mut("data") {
        crate::computed_fields::strip(w, &kind, data);
    }
    Ok(())
}

fn merge_data(w: &Workspace, value: &mut Value, clean: bool) -> Result<()> {
    let (Some(kind), Some(id)) = (value["kind"].as_str(), value["id"].as_str()) else {
        return Ok(());
    };
    if value["revision"].as_u64().unwrap_or(0) == 0 {
        return Ok(());
    }
    // An unknown object or a stale revision is refused later, by the save itself.
    let Some(object) = w.objects.get(resolve_address(w, &address(kind, id))) else {
        return Ok(());
    };
    let current = object.current();
    if value.get("parent").is_none() {
        value["parent"] = current.parent.clone().map_or(Value::Null, Value::String);
    }
    if clean {
        return Ok(());
    }
    let (Some(sent), Some(stored)) = (value["data"].as_object(), current.data.as_object()) else {
        return Ok(());
    };
    let mut data = stored.clone();
    // A null removes a stored field. A null of an absent field stays, so the checks of the save
    // read it as sent.
    for (field, sent) in sent {
        if sent.is_null() && data.contains_key(field) {
            data.remove(field);
        } else {
            data.insert(field.clone(), sent.clone());
        }
    }
    value["data"] = Value::Object(data);
    Ok(())
}
