//! joppa_help: a short table of contents, or one section, or the shape of one operation.
use anyhow::{Context, bail, ensure};
use deadpool_postgres::Pool;
use joppa::{
    commands::{OPERATIONS, OPTIONAL_FIELDS},
    store,
};
use serde_json::{Map, Value, json};

/// Each section and what it gives. The sections tree and examples need a workspace.
const SECTIONS: [(&str, &str); 6] = [
    (
        "operations",
        "The required fields and the callers of each operation. Supply op to read one operation.",
    ),
    ("writing", "The ASD-STE100 rules for new technical text."),
    (
        "objects",
        "The object model: types, typed and computed fields, links, comments, tests and archives.",
    ),
    (
        "search",
        "How joppa_search finds, ranks and filters objects.",
    ),
    (
        "tree",
        "The tree schema of the workspace with the type questions. Needs workspace.",
    ),
    (
        "examples",
        "One save_object example for each type of the workspace. Needs workspace.",
    ),
];

const HINT: &str = "Supply section to read one section, or op to read one operation.";

/// The help that `section` or `op` names. Without them, the table of contents.
pub(super) async fn help(pool: &Pool, actor: &str, a: &Value) -> anyhow::Result<Value> {
    let text = |key: &str| match &a[key] {
        Value::Null => Ok(None),
        value => value
            .as_str()
            .map(Some)
            .with_context(|| format!("{key} must be a string")),
    };
    let (section, op) = (text("section")?, text("op")?);
    ensure!(
        op.is_none() || section.is_none_or(|s| s == "operations"),
        "op reads the operations section"
    );
    let Some(section) = section.or(op.map(|_| "operations")) else {
        let sections: Map<String, Value> = SECTIONS
            .iter()
            .map(|(name, text)| ((*name).to_owned(), json!(text)))
            .collect();
        let ops: Vec<&str> = OPERATIONS.iter().map(|o| o.name).collect();
        return Ok(json!({"sections": sections, "ops": ops, "hint": HINT}));
    };
    let value = match section {
        "operations" => {
            let operations: Map<String, Value> = OPERATIONS
                .iter()
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
        "tree" | "examples" => {
            let workspace =
                text("workspace")?.with_context(|| format!("section {section} needs workspace"))?;
            let tree = store::tree_schema(pool, workspace, actor, None).await?;
            match section {
                "tree" => tree,
                _ => {
                    let types = serde_json::from_value(tree["schema"]["types"].clone())?;
                    joppa::save_examples::save_examples(&types)
                }
            }
        }
        _ => bail!("unknown section {section}; read joppa_help without section for the list"),
    };
    let mut reply = Map::new();
    reply.insert(section.to_owned(), value);
    Ok(Value::Object(reply))
}
