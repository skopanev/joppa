//! A live save_object of an existing object merges its data into the current data: a sent
//! field replaces its value, a null removes a stored field, and every other field stays. An omitted
//! parent keeps the current parent. An update without data keeps the stored data.
//! `clean: true` replaces the data with the sent data. The
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

/// An update (revision > 0) without data and without `clean: true` keeps the stored data: the
/// command gets the data `{}`, and the merge fills it. A create or a clean save still needs data.
pub fn default_data(value: &mut Value, clean: bool) {
    let update = value["revision"].as_u64().is_some_and(|r| r > 0);
    if value["op"] == "save_object" && update && !clean && value.get("data").is_none() {
        value["data"] = Value::Object(Default::default());
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
    relations_in_data(w, &kind, &value["data"])?;
    merge_data(w, value, clean)?;
    if let Some(data) = value.get_mut("data") {
        crate::computed_fields::strip(w, &kind, data);
    }
    Ok(())
}

/// A live save sends relations at the top level. A sent data.relations or data.links that is
/// not null is refused, unless the type declares a typed field of that name. A null removes an
/// older stored value, and the stored data of an older event stays.
fn relations_in_data(w: &Workspace, kind: &str, data: &Value) -> Result<()> {
    let sent = |field: &str| data.get(field).is_some_and(|v| !v.is_null());
    if !sent("relations") && !sent("links") {
        return Ok(());
    }
    let schema = crate::objects::schema(w);
    let declared = |field: &str| {
        let t = schema.types.get(kind);
        t.is_some_and(|t| t.fields.contains_key(field))
    };
    for field in ["relations", "links"] {
        if sent(field) && !declared(field) {
            bail!(
                "data.{field} is not a data field of {kind}; send relations at the top level of save_object"
            );
        }
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

#[cfg(test)]
#[path = "save_merge_tests.rs"]
mod tests;
