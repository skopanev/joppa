//! Part of `commands`: `operation` and the items after it, moved from commands.rs unchanged.
use super::*;

/// One public operation of joppa_command and joppa_batch.
/// `required` lists the fields that the caller must send. Generated identifiers,
/// optional fields and fields with defaults are absent from it.
/// `callers` uses member and human member. Value checks, such as
/// REQ ownership or a person decision, still apply inside each category.
pub struct Operation {
    pub name: &'static str,
    pub required: &'static [&'static str],
    pub callers: &'static [&'static str],
}

pub(crate) const MEMBER: &[&str] = &["member"];

pub(crate) const HUMAN_MEMBER: &[&str] = &["human member"];

pub(crate) const fn op(
    name: &'static str,
    required: &'static [&'static str],
    callers: &'static [&'static str],
) -> Operation {
    Operation {
        name,
        required,
        callers,
    }
}

/// The public operation catalog. Help, the command schemas and remote admission use it.
/// Internal, retired and CLI-only operations are absent.
pub const OPERATIONS: &[Operation] = &[
    op("set_tree_schema", &["revision", "reason"], MEMBER),
    op(
        "save_object",
        &["kind", "revision", "data", "reason"],
        MEMBER,
    ),
    op("archive_object", &["target", "revision", "reason"], MEMBER),
    op(
        "set_remote_links",
        &["target", "revision", "links", "reason"],
        MEMBER,
    ),
    op("restore_object", &["target", "revision", "reason"], MEMBER),
    op(
        "redact_object",
        &["target", "fragment", "reason"],
        HUMAN_MEMBER,
    ),
    op("update_workspace", &[], HUMAN_MEMBER),
    op("delete_workspace", &["reason"], HUMAN_MEMBER),
    op("reorder_tree", &["kind", "expected", "ids"], MEMBER),
    op("member", &["id", "role"], HUMAN_MEMBER),
    op("remove_member", &["id", "reason"], HUMAN_MEMBER),
    op("note", &["target", "id", "text"], MEMBER),
    op("withdraw_note", &["target", "id", "reason"], MEMBER),
    op(
        "attach",
        &["target", "id", "name", "content_type", "size", "hash"],
        MEMBER,
    ),
    op("detach", &["target", "id", "reason"], MEMBER),
];

/// The optional fields of an operation with their allowed values, as help gives them.
pub const OPTIONAL_FIELDS: &[(&str, &[(&str, &str)])] = &[
    (
        "attach",
        &[
            (
                "check_result",
                "pass or fail: a manual check of a criterion",
            ),
            (
                "valid_until",
                "YYYY-MM-DD (UTC), with check_result: the day a pass expires",
            ),
        ],
    ),
    (
        "save_object",
        &[(
            "relations",
            "{key: {title?, color?, facts?, edges: [{to, when?, revision?, color?}]}} by the keys \
             of schema.relations: a sent key replaces its title, edges, color and facts, null \
             removes it, other keys stay; color is a palette name or #rrggbb, facts are objects; \
             depends_on blocks Ready, related and a flow block nothing",
        )],
    ),
    (
        "set_tree_schema",
        &[
            (
                "put_relations",
                "{key: {title, description?}}: adds a relation or replaces its texts",
            ),
            (
                "remove_relations",
                "[key]: removes a relation that no live node holds; not depends_on or related",
            ),
        ],
    ),
];

pub fn operation(name: &str) -> Option<&'static Operation> {
    OPERATIONS.iter().find(|o| o.name == name)
}

pub(crate) fn verification_identifier(w: &Workspace, id: &str, role: &str) -> Result<()> {
    if let Some((kind, local)) = id.split_once(':') {
        identifier(kind)?;
        ensure!(
            !local.is_empty()
                && local.len() <= 200
                && !local.contains(':')
                && !local.chars().any(char::is_whitespace),
            "invalid object address"
        );
        ensure!(
            crate::objects::schema(w)
                .types
                .get(kind)
                .is_some_and(|t| t.role == role),
            "object type has no {role} role"
        );
        Ok(())
    } else {
        identifier(id)
    }
}

pub fn nonempty(s: &str) -> Result<()> {
    ensure!(!s.trim().is_empty(), "text must not be empty");
    Ok(())
}

/// A field that the client must not send. HTTP answers it with 422.
#[derive(Debug)]
pub struct ClientField(pub &'static str);

impl std::fmt::Display for ClientField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ClientField {}

pub fn reject_actor_field(input: &serde_json::Value) -> Result<()> {
    if input.get("actor").is_some() {
        return Err(ClientField("actor comes from the identity, not the request").into());
    }
    Ok(())
}

pub fn identifier(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty()
            && s.len() <= 120
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "invalid id: {s}"
    );
    Ok(())
}

/// The limit of a description: the same 1024 characters as each object field.
pub const DESCRIPTION_MAX: usize = crate::field_limit::FIELD_MAX;

/// Проверка адреса объекта: `вид:идентификатор`.
///
/// Адрес проверяется на СУЩЕСТВОВАНИЕ объекта, а не только на форму. Иначе
/// комментарий уезжал бы в никуда и находился бы только тем, кто угадал ту же
/// опечатку: заметка на несуществующем домене не видна нигде.
pub fn target(w: &Workspace, address: &str) -> Result<()> {
    let address = crate::objects::resolve_address(w, address);
    if let Some(object) = w.objects.get(address) {
        ensure!(
            object.current().present,
            "object is absent from the current tree"
        );
        return Ok(());
    }
    let (kind, rest) = address
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("address looks like kind:id, for example domain:billing"))?;
    match kind {
        "domain" => ensure!(w.domains.contains_key(rest), "no such domain: {rest}"),
        "capability" => ensure!(
            w.capabilities.contains_key(rest),
            "no such capability: {rest}"
        ),
        "consumer" => ensure!(w.consumers.contains_key(rest), "no such consumer: {rest}"),
        "requirement" => ensure!(
            w.requirements.contains_key(rest),
            "no such requirement: {rest}"
        ),
        "ac" => {
            let (req, ac) = rest.split_once('/').ok_or_else(|| {
                anyhow::anyhow!("an acceptance criterion is ac:<requirement>/<id>")
            })?;
            let r = w
                .requirements
                .get(req)
                .and_then(|r| r.revisions.last())
                .ok_or_else(|| anyhow::anyhow!("no such requirement: {req}"))?;
            ensure!(
                r.acs.iter().any(|a| a.id == ac),
                "no such acceptance criterion in the current revision: {ac}"
            );
        }
        "intake" => ensure!(w.intake.contains_key(rest), "no such intake: {rest}"),
        "point" => {
            let (intake, point) = rest
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("an intake point is point:<intake>/<id>"))?;
            let i = w
                .intake
                .get(intake)
                .ok_or_else(|| anyhow::anyhow!("no such intake: {intake}"))?;
            ensure!(i.points.contains_key(point), "no such point: {point}");
        }
        other => bail!(
            "unknown kind {other}: address a domain, capability, consumer, requirement, ac, intake or point"
        ),
    }
    Ok(())
}
