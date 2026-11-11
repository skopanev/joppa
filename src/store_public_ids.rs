//! A write takes a public item ID, such as `joppa-R-DP6KXM62`, in each field that names an
//! object, also in an edge target and a fact of a relation. The server replaces it with the canonical `kind:id` before the validation, so the
//! journal and the documents store only addresses.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// A public item ID: a prefix, one type letter and a code of 8 or more `A`–`Z` and `0`–`9`.
/// An address has `:`, so it is never a public item ID.
pub fn public_item_id(key: &str) -> bool {
    let Some((head, code)) = key.rsplit_once('-') else {
        return false;
    };
    let Some((prefix, letter)) = head.rsplit_once('-') else {
        return false;
    };
    let upper = |b: u8| b.is_ascii_uppercase() || b.is_ascii_digit();
    code.len() >= 8
        && code.bytes().all(upper)
        && letter.len() == 1
        && letter.bytes().all(|b| b.is_ascii_uppercase())
        && !prefix.is_empty()
        && prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// The JSON pointer of each field of a command that names an object of the workspace. A save
/// also names objects in the reference fields of its type. The targets of `set_remote_links`
/// are objects of other workspaces, so they stay as sent.
fn slots(state: &Workspace, value: &Value) -> Vec<String> {
    let mut out = vec![];
    for field in ["target", "parent", "duplicate_of"] {
        if value[field].is_string() {
            out.push(format!("/{field}"));
        }
    }
    // The targets of set_remote_links are in other workspaces, and save_object refuses links.
    let links = value["links"].as_array().filter(|_| {
        !matches!(
            value["op"].as_str(),
            Some("set_remote_links" | "save_object")
        )
    });
    for (i, link) in links.into_iter().flatten().enumerate() {
        if link.is_string() {
            out.push(format!("/links/{i}"));
        } else if link["target"].is_string() {
            out.push(format!("/links/{i}/target"));
        }
    }
    if value["op"] == "save_object" {
        // Each edge of each relation names its target in `to`, and each fact is an object.
        for (key, relation) in value["relations"].as_object().into_iter().flatten() {
            let key = key.replace('~', "~0").replace('/', "~1");
            for (i, edge) in relation["edges"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if edge["to"].is_string() {
                    out.push(format!("/relations/{key}/edges/{i}/to"));
                }
            }
            let facts = relation["facts"].as_array().into_iter().flatten();
            for (i, _) in facts.enumerate().filter(|(_, fact)| fact.is_string()) {
                out.push(format!("/relations/{key}/facts/{i}"));
            }
        }
        let kind = value["kind"].as_str().unwrap_or_default();
        let schema = crate::objects::schema(state);
        let fields = schema.types.get(kind).map(|t| t.references.keys());
        for field in fields.into_iter().flatten() {
            match &value["data"][field] {
                Value::String(_) => out.push(format!("/data/{field}")),
                Value::Array(values) => {
                    out.extend((0..values.len()).map(|i| format!("/data/{field}/{i}")))
                }
                _ => {}
            }
        }
        if value["id"].is_string() {
            out.push("/id".into());
        }
    }
    out
}

/// The address of each public item ID of one workspace. An unknown ID is refused.
pub(crate) async fn addresses(
    client: &impl GenericClient,
    workspace: &str,
    keys: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let keys: Vec<&String> = keys.iter().collect();
    let rows = client
        .query(
            "SELECT k.id,i.kind||':'||i.local_id FROM unnest($2::text[]) k(id)
             LEFT JOIN joppa.item_ids i ON i.workspace=$1 AND i.id=joppa.short_item_id(k.id)",
            &[&workspace, &keys],
        )
        .await?;
    let mut out = BTreeMap::new();
    for row in rows {
        let key: String = row.get(0);
        let address: Option<String> = row.get(1);
        let address = address.with_context(|| format!("unknown object: {key}"))?;
        out.insert(key, address);
    }
    Ok(out)
}

/// The address of each node key: an address stays, a public item ID becomes its address.
pub(crate) async fn node_addresses(
    client: &impl GenericClient,
    workspace: &str,
    keys: &[String],
) -> Result<Vec<String>> {
    let ids: BTreeSet<String> = keys.iter().filter(|k| public_item_id(k)).cloned().collect();
    if ids.is_empty() {
        return Ok(keys.to_vec());
    }
    let found = addresses(client, workspace, &ids).await?;
    Ok(keys
        .iter()
        .map(|k| found.get(k).unwrap_or(k).clone())
        .collect())
}

/// The `id` of save_object in the form `kind:id` names the kind of the object: a different kind
/// is refused, and the command keeps the local part. A local part with `:` or without a character
/// stays, so the save refuses it with "invalid object id".
pub(crate) fn local_id(value: &mut Value) -> Result<()> {
    let id = value["id"]
        .as_str()
        .filter(|_| value["op"] == "save_object");
    let Some((kind, local)) = id.and_then(|id| id.split_once(':')) else {
        return Ok(());
    };
    let wanted = value["kind"].as_str().unwrap_or_default();
    ensure!(
        kind == wanted,
        "the id names an object of type {kind}, not {wanted}"
    );
    value["id"] = json!(local.to_owned());
    Ok(())
}

/// Replace each public item ID of a command with its address. A public item ID in the `id`
/// of `save_object` names the object to edit and must have its kind.
pub(crate) async fn canonical(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    value: &mut Value,
) -> Result<()> {
    local_id(value)?;
    let slots = slots(state, value);
    let ids: BTreeSet<String> = slots
        .iter()
        .filter_map(|p| value.pointer(p)?.as_str())
        .filter(|k| public_item_id(k))
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let found = addresses(client, workspace, &ids).await?;
    for pointer in &slots {
        let Some(slot) = value.pointer_mut(pointer) else {
            continue;
        };
        let Some(address) = slot.as_str().and_then(|k| found.get(k)).cloned() else {
            continue;
        };
        if pointer != "/id" {
            *slot = json!(address);
            continue;
        }
        let (kind, local) = address.split_once(':').context("an address has a kind")?;
        let wanted = value["kind"].as_str().unwrap_or_default();
        ensure!(
            kind == wanted,
            "the item ID names an object of type {kind}, not {wanted}"
        );
        value["id"] = json!(local);
    }
    Ok(())
}

#[cfg(test)]
#[path = "store_public_ids_tests.rs"]
mod tests;
