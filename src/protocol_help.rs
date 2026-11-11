//! joppa_help: a short table of contents, or one section, or the shape of one operation.
use anyhow::{Context, bail, ensure};
use deadpool_postgres::Pool;
use joppa::{
    commands::{OPTIONAL_FIELDS, mcp_operations},
    store,
};
use serde_json::{Map, Value, json};

/// Each section and what it gives. The section examples needs a workspace.
const SECTIONS: [(&str, &str); 6] = [
    (
        "operations",
        "The required fields and the callers of each operation. Supply op to read one operation.",
    ),
    ("writing", "The ASD-STE100 rules for new technical text."),
    (
        "objects",
        "The object model: types, typed and computed fields, relations, comments, tests and archives.",
    ),
    (
        "search",
        "How joppa_search finds, ranks and filters objects.",
    ),
    (
        "process",
        "The work rules: Inbox items, Sources, the 11111 checklist, the Reviewer, Tasks and tests.",
    ),
    (
        "examples",
        "One save_object example for each type of the workspace. Needs workspace; kind gives one type.",
    ),
];

const HINT: &str = "Supply section to read one section, or op to read one operation.";

/// The rules of the work: Inbox items, Sources, scores, Tasks and tests.
const PROCESS_RULES: &str = include_str!("../docs/agent-process.txt");

/// The help that `section`, `op` or `kind` names. Without them, the table of contents.
pub(super) async fn help(pool: &Pool, actor: &str, a: &Value) -> anyhow::Result<Value> {
    let text = |key: &str| match &a[key] {
        Value::Null => Ok(None),
        value => value
            .as_str()
            .map(Some)
            .with_context(|| format!("{key} must be a string")),
    };
    let (section, op, kind) = (text("section")?, text("op")?, text("kind")?);
    ensure!(
        op.is_none() || section.is_none_or(|s| s == "operations"),
        "op reads the operations section"
    );
    ensure!(
        kind.is_none() || section.is_none_or(|s| s == "examples"),
        "kind reads the examples section"
    );
    let implied = op.map(|_| "operations").or(kind.map(|_| "examples"));
    let Some(section) = section.or(implied) else {
        let sections: Map<String, Value> = SECTIONS
            .iter()
            .map(|(name, text)| ((*name).to_owned(), json!(text)))
            .collect();
        let ops: Vec<&str> = mcp_operations().map(|o| o.name).collect();
        return Ok(json!({"sections": sections, "ops": ops, "hint": HINT}));
    };
    let value = match section {
        "operations" => {
            let operations: Map<String, Value> = mcp_operations()
                .filter(|o| op.is_none_or(|name| name == o.name))
                .map(|o| {
                    let mut shape = json!({"required": o.required, "callers": o.callers});
                    let optional = OPTIONAL_FIELDS.iter().find(|(name, _)| *name == o.name);
                    if let Some((_, fields)) = optional {
                        let fields: Map<String, Value> = fields
                            .iter()
                            .map(|(f, v)| ((*f).into(), json!(v)))
                            .collect();
                        shape["optional"] = Value::Object(fields);
                    }
                    (o.name.to_owned(), shape)
                })
                .collect();
            if let Some(op) = op {
                ensure!(!operations.is_empty(), "unknown operation {op}");
            }
            Value::Object(operations)
        }
        "writing" => json!(super::WRITING_RULES),
        "objects" => json!(include_str!("../docs/objects.md")),
        "search" => json!(include_str!("../docs/search.md")),
        "process" => json!(PROCESS_RULES),
        "tree" => bail!("section tree was removed; use joppa_tree"),
        "examples" => {
            let workspace = text("workspace")?.context("section examples needs workspace")?;
            let tree = store::tree_schema(pool, workspace, actor, None).await?;
            let mut types: std::collections::BTreeMap<String, joppa::objects::TreeType> =
                serde_json::from_value(tree["schema"]["types"].clone())?;
            if let Some(kind) = kind {
                ensure!(types.contains_key(kind), "unknown type {kind}");
                types.retain(|name, _| name == kind);
            }
            joppa::save_examples::save_examples(&types)
        }
        _ => bail!("unknown section {section}; read joppa_help without section for the list"),
    };
    let mut reply = Map::new();
    reply.insert(section.to_owned(), value);
    Ok(Value::Object(reply))
}
