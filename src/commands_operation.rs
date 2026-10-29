//! Part of `commands`: `operation` and the items after it, moved from commands.rs unchanged.
use super::*;

/// One public operation of joppa_command and joppa_batch.
/// `required` lists the fields that the caller must send. Generated identifiers,
/// optional fields and fields with defaults are absent from it.
/// `callers` uses member, agent, holder and workspace owner. Value checks, such as
/// REQ ownership or a person decision, still apply inside each category.
pub struct Operation {
    pub name: &'static str,
    pub required: &'static [&'static str],
    pub callers: &'static [&'static str],
}

pub(crate) const MEMBER: &[&str] = &["member"];

pub(crate) const OWNER: &[&str] = &["workspace owner"];

pub(crate) const HOLDER: &[&str] = &["holder"];

pub(crate) const HOLDER_OR_OWNER: &[&str] = &["holder", "workspace owner"];

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
    op("set_ac_tests", &["target", "tests", "reason"], MEMBER),
    op(
        "decide_note",
        &["target", "revision", "outcome", "reason"],
        MEMBER,
    ),
    op("accept_commit", &["target", "revision", "commit"], MEMBER),
    op("redact_object", &["target", "fragment", "reason"], OWNER),
    op("rename_workspace", &["title", "reason"], OWNER),
    op("set_priority", &["target", "priority"], MEMBER),
    op("assess_signal", &["target", "ready", "reason"], MEMBER),
    op("delete_workspace", &["reason"], OWNER),
    op("reorder_tree", &["kind", "expected", "ids"], MEMBER),
    op("member", &["id", "role"], OWNER),
    op("remove_member", &["id", "reason"], OWNER),
    op("confirm", &["req", "revision"], MEMBER),
    op("comment", &["req", "revision", "text", "kind"], MEMBER),
    op("note", &["target", "id", "text"], MEMBER),
    op("withdraw_note", &["target", "id", "reason"], MEMBER),
    op(
        "attach",
        &["target", "id", "name", "content_type", "size", "hash"],
        MEMBER,
    ),
    op("detach", &["target", "id", "reason"], MEMBER),
    op("assess", &["req", "revision", "ac", "assessment"], MEMBER),
    op("claim_planning", &["ac", "revision"], MEMBER),
    op("finish_planning", &["ac", "reason"], HOLDER),
    op("release_planning", &["ac", "reason"], HOLDER_OR_OWNER),
    op("claim_task", &["revision", "id"], MEMBER),
    // Public alias: the server resolves it to claim_task under the write lock.
    op("claim_next_task", &[], MEMBER),
    op(
        "release_task",
        &["revision", "id", "reason"],
        HOLDER_OR_OWNER,
    ),
    op(
        "block_task",
        &["revision", "id", "reason", "evidence"],
        HOLDER_OR_OWNER,
    ),
    op(
        "unblock_task",
        &["revision", "id", "reason", "evidence"],
        HOLDER_OR_OWNER,
    ),
    op(
        "complete_task",
        &["revision", "id", "evidence", "commit"],
        HOLDER,
    ),
    op("review_task", &["revision", "id", "reason"], MEMBER),
    op("retire_task", &["revision", "id", "reason"], MEMBER),
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
