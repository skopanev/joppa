use crate::{checks, model::*};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    MigratePersistentClaims {},
    /// Internal migration event. Public authoring rejects this operation.
    MigrateNotes {},
    MigrateTasks {},
    MigrateSchema {},
    MigrateDocuments {},
    MigrateTaskClaims {},
    SetTreeSchema {
        revision: u32,
        types: BTreeMap<String, crate::objects::TreeType>,
        reason: String,
    },
    SaveObject {
        id: String,
        kind: String,
        revision: u32,
        parent: Option<String>,
        data: serde_json::Value,
        #[serde(default)]
        links: Vec<crate::objects::ObjectLink>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        question_checked: Option<String>,
        reason: String,
    },
    ArchiveObject {
        target: String,
        revision: u32,
        reason: String,
    },
    RestoreObject {
        target: String,
        revision: u32,
        reason: String,
    },
    /// Save the list of tests that prove a criterion. It keeps its own history.
    SetAcTests {
        target: String,
        tests: Vec<String>,
        /// The list revision the author read; 0 means no saved list. Absent skips the check.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<u32>,
        reason: String,
    },
    /// Decide an Inbox signal: changed_spec, needs_information, rejected or duplicate.
    DecideNote {
        target: String,
        revision: u32,
        outcome: String,
        reason: String,
        #[serde(default)]
        links: Vec<String>,
        #[serde(default)]
        duplicate_of: Option<String>,
        #[serde(default)]
        question: Option<String>,
        #[serde(default)]
        recipient: Option<String>,
    },
    AcceptCommit {
        target: String,
        revision: u32,
        #[serde(default)]
        commit: Option<String>,
    },
    RedactObject {
        target: String,
        fragment: String,
        reason: String,
    },
    SetNoteStatus {
        target: String,
        revision: u32,
        status: String,
        reason: String,
    },
    Workspace {
        title: String,
    },
    RenameWorkspace {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
        title: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference_prefix: Option<String>,
    },
    ReorderTree {
        kind: String,
        parent: Option<String>,
        expected: Vec<String>,
        ids: Vec<String>,
    },
    Member {
        id: String,
        role: String,
    },
    Consumer {
        id: String,
        title: String,
        #[serde(default)]
        description: String,
        owner: String,
    },
    EditConsumer {
        id: String,
        title: String,
        description: String,
        owner: String,
        reason: String,
    },
    /// Internal enrollment event; public command entry points reject this operation.
    EnrollMember {
        id: String,
    },
    Domain {
        id: String,
        title: String,
        #[serde(alias = "authority")]
        owner: String,
        #[serde(default)]
        description: String,
    },
    Capability {
        id: String,
        title: String,
        domain: Option<String>,
        #[serde(default)]
        owner: Option<String>,
        #[serde(default)]
        description: String,
    },
    SetDomainArchived {
        id: String,
        archived: bool,
        reason: String,
    },
    ConfirmTree {},
    EditDomain {
        id: String,
        title: String,
        #[serde(alias = "authority")]
        owner: String,
        #[serde(default)]
        description: String,
        reason: String,
    },
    EditCapability {
        id: String,
        title: String,
        #[serde(default)]
        owner: Option<String>,
        #[serde(default)]
        description: String,
        reason: String,
    },
    WithdrawDecision {
        req: String,
        revision: u32,
        comment: String,
        reason: String,
    },
    Project {
        id: String,
        repository: String,
    },
    Revise {
        req: String,
        expected_revision: u32,
        capability: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        consumers: Vec<String>,
        title: String,
        body: String,
        owner: String,
        reason: String,
        acs: Vec<Ac>,
        /// Persisted compatibility marker: old journal revisions predate
        /// cross-revision coverage review and remain replayable unchanged.
        #[serde(default)]
        coverage_policy: bool,
        #[serde(default)]
        carry_tasks: bool,
    },
    ReviewCoverage {
        req: String,
        revision: u32,
        run: String,
        reason: String,
    },
    DefineVerificationScope {
        req: String,
        revision: u32,
        subject_keys: Vec<String>,
        scope: String,
        reason: String,
    },
    LinkSource {
        req: String,
        revision: u32,
        id: String,
        kind: String,
        #[serde(default)]
        uri: String,
        #[serde(default)]
        external_id: Option<String>,
        version: String,
        relation: String,
    },
    WithdrawSource {
        req: String,
        revision: u32,
        id: String,
        reason: String,
    },
    Confirm {
        req: String,
        revision: u32,
    },
    Comment {
        req: String,
        revision: u32,
        id: String,
        text: String,
        kind: String,
        reply_to: Option<String>,
        ac: Option<String>,
    },
    /// Пояснение к объекту, у которого нет своего поля описания.
    /// У домена, капабилити и AC оно есть — там его правят вместе с самим
    /// объектом, чтобы описание менялось одной ревизией с тем, что описывает.
    Describe {
        target: String,
        text: String,
        reason: String,
    },
    /// Свободный комментарий на любом объекте.
    ///
    /// Не путать с `Comment`: тот — процессный журнал требования, он несёт
    /// ревизию, вид и силу решения. Здесь — обычная человеческая реплика,
    /// какую вешают на домен или на пункт интейка.
    Note {
        target: String,
        id: String,
        text: String,
        #[serde(default)]
        reply_to: Option<String>,
    },
    WithdrawNote {
        target: String,
        id: String,
        reason: String,
    },
    /// Вложение. Сами байты уже лежат в хранилище — сюда едет отпечаток.
    Attach {
        target: String,
        id: String,
        name: String,
        content_type: String,
        size: u64,
        hash: String,
    },
    /// Снятие вложения. Файл из хранилища не удаляется: он мог быть
    /// приложен и в другом месте, а ссылка из истории должна открываться.
    Detach {
        target: String,
        id: String,
        reason: String,
    },
    Assess {
        req: String,
        revision: u32,
        ac: String,
        assessment: Assessment,
    },
    Task {
        req: String,
        revision: u32,
        id: String,
        title: String,
        acs: Vec<String>,
        projects: Vec<String>,
        purpose: String,
        #[serde(default)]
        assignee: Option<String>,
        #[serde(default)]
        contract: String,
    },
    ClaimPlanning {
        ac: String,
        revision: u32,
    },
    FinishPlanning {
        ac: String,
        reason: String,
    },
    ReleasePlanning {
        ac: String,
        reason: String,
    },
    HeartbeatTask {
        revision: u32,
        id: String,
    },
    ClaimTask {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        req: String,
        revision: u32,
        id: String,
    },
    AmendTask {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        req: String,
        revision: u32,
        id: String,
        title: String,
        contract: String,
        projects: Vec<String>,
        reason: String,
    },
    ReleaseTask {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        req: String,
        revision: u32,
        id: String,
        reason: String,
    },
    BlockTask {
        revision: u32,
        id: String,
        reason: String,
        evidence: String,
    },
    UnblockTask {
        revision: u32,
        id: String,
        reason: String,
        evidence: String,
    },
    CompleteTask {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        req: String,
        revision: u32,
        id: String,
        evidence: String,
    },
    ReviewTask {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        req: String,
        revision: u32,
        id: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        ac: String,
        reason: String,
    },
    RetireTask {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        req: String,
        revision: u32,
        id: String,
        reason: String,
    },
    Plan {
        req: String,
        revision: u32,
        run: String,
        subject: BTreeMap<String, String>,
        scope: String,
        discovery_complete: bool,
        checks: Vec<Check>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        enforce_coverage: bool,
    },
    Result {
        req: String,
        revision: u32,
        run: String,
        evidence: Evidence,
    },
    Finish {
        req: String,
        revision: u32,
        run: String,
    },
    Accept {
        req: String,
        revision: u32,
        run: String,
    },
    Revoke {
        req: String,
        revision: u32,
        reason: String,
    },
    Issue {
        req: String,
        revision: u32,
        id: String,
        ac: String,
        comment: String,
    },
    Resolve {
        req: String,
        revision: u32,
        id: String,
        comment: String,
    },
    Intake {
        id: String,
        said: String,
        author: String,
        #[serde(default)]
        owner: Option<String>,
    },
    Split {
        id: String,
        points: Vec<String>,
    },
    Questions {
        kind: String,
        fields: Vec<String>,
    },
    Sort {
        id: String,
        point: String,
        kind: String,
    },
    Answer {
        id: String,
        point: String,
        field: String,
        value: String,
    },
    Route {
        id: String,
        point: String,
        outcome: String,
        reason: String,
        req: Option<String>,
    },
}

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

const MEMBER: &[&str] = &["member"];
const OWNER: &[&str] = &["workspace owner"];
const AGENT_OR_OWNER: &[&str] = &["agent", "workspace owner"];
const HOLDER: &[&str] = &["holder"];
const HOLDER_OR_OWNER: &[&str] = &["holder", "workspace owner"];

const fn op(
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
    op(
        "set_tree_schema",
        &["revision", "types", "reason"],
        AGENT_OR_OWNER,
    ),
    op(
        "save_object",
        &["kind", "revision", "data", "reason"],
        MEMBER,
    ),
    op(
        "archive_object",
        &["target", "revision", "reason"],
        AGENT_OR_OWNER,
    ),
    op(
        "restore_object",
        &["target", "revision", "reason"],
        AGENT_OR_OWNER,
    ),
    op("set_ac_tests", &["target", "tests", "reason"], MEMBER),
    op(
        "decide_note",
        &["target", "revision", "outcome", "reason"],
        MEMBER,
    ),
    op("accept_commit", &["target", "revision", "commit"], MEMBER),
    op("redact_object", &["target", "fragment", "reason"], OWNER),
    op(
        "set_note_status",
        &["target", "revision", "status", "reason"],
        MEMBER,
    ),
    op("rename_workspace", &["title", "reason"], OWNER),
    op("reorder_tree", &["kind", "expected", "ids"], OWNER),
    op("member", &["id", "role"], OWNER),
    op("consumer", &["title", "owner"], MEMBER),
    op(
        "edit_consumer",
        &["id", "title", "description", "owner", "reason"],
        MEMBER,
    ),
    op("domain", &["title", "owner"], MEMBER),
    op("capability", &["title"], MEMBER),
    op("set_domain_archived", &["id", "archived", "reason"], OWNER),
    op("confirm_tree", &[], OWNER),
    op(
        "edit_domain",
        &["id", "title", "owner", "reason"],
        AGENT_OR_OWNER,
    ),
    op(
        "edit_capability",
        &["id", "title", "reason"],
        AGENT_OR_OWNER,
    ),
    op(
        "withdraw_decision",
        &["req", "revision", "comment", "reason"],
        MEMBER,
    ),
    op("project", &["repository"], MEMBER),
    op(
        "revise",
        &[
            "expected_revision",
            "title",
            "body",
            "owner",
            "reason",
            "acs",
        ],
        MEMBER,
    ),
    op(
        "review_coverage",
        &["req", "revision", "run", "reason"],
        MEMBER,
    ),
    op(
        "define_verification_scope",
        &["req", "revision", "subject_keys", "scope", "reason"],
        MEMBER,
    ),
    op(
        "link_source",
        &["req", "revision", "kind", "version", "relation"],
        MEMBER,
    ),
    op(
        "withdraw_source",
        &["req", "revision", "id", "reason"],
        MEMBER,
    ),
    op("confirm", &["req", "revision"], MEMBER),
    op("comment", &["req", "revision", "text", "kind"], MEMBER),
    op("describe", &["target", "text", "reason"], MEMBER),
    op("note", &["target", "id", "text"], MEMBER),
    op("withdraw_note", &["target", "id", "reason"], MEMBER),
    op(
        "attach",
        &["target", "id", "name", "content_type", "size", "hash"],
        MEMBER,
    ),
    op("detach", &["target", "id", "reason"], MEMBER),
    op("assess", &["req", "revision", "ac", "assessment"], MEMBER),
    op(
        "task",
        &[
            "req", "revision", "title", "acs", "projects", "purpose", "contract",
        ],
        MEMBER,
    ),
    op("claim_planning", &["ac", "revision"], MEMBER),
    op("finish_planning", &["ac", "reason"], HOLDER),
    op("release_planning", &["ac", "reason"], HOLDER_OR_OWNER),
    op("claim_task", &["revision", "id"], MEMBER),
    // Public alias: the server resolves it to claim_task under the write lock.
    op("claim_next_task", &[], MEMBER),
    op(
        "amend_task",
        &["revision", "id", "title", "contract", "projects", "reason"],
        MEMBER,
    ),
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
    op("complete_task", &["revision", "id", "evidence"], HOLDER),
    op("review_task", &["revision", "id", "reason"], MEMBER),
    op("retire_task", &["revision", "id", "reason"], MEMBER),
    op(
        "plan",
        &[
            "req",
            "revision",
            "subject",
            "scope",
            "discovery_complete",
            "checks",
        ],
        MEMBER,
    ),
    op("result", &["req", "revision", "run", "evidence"], MEMBER),
    op("finish", &["req", "revision", "run"], MEMBER),
    op("accept", &["req", "revision", "run"], MEMBER),
    op("revoke", &["req", "revision", "reason"], MEMBER),
    op("issue", &["req", "revision", "ac", "comment"], MEMBER),
    op("resolve", &["req", "revision", "id", "comment"], MEMBER),
];

pub fn operation(name: &str) -> Option<&'static Operation> {
    OPERATIONS.iter().find(|o| o.name == name)
}

fn verification_identifier(w: &Workspace, id: &str, role: &str) -> Result<()> {
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
/// Предел пояснения. Две тысячи знаков — около страницы: достаточно, чтобы
/// объяснить границу и оговорки, и мало, чтобы вместо пояснения написать сюда
/// само требование. Предел объявлен здесь одним числом, потому что он входит
/// и в проверку, и в описание инструмента, и в подсказку в интерфейсе.
pub const DESCRIPTION_MAX: usize = 2000;

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
/// Decisions on requirements come from people; an agent may only propose them.
pub fn person(w: &Workspace, actor: &str) -> Result<()> {
    ensure!(
        crate::inbox::actor_type(w, actor) == "person",
        "only a person records this decision"
    );
    Ok(())
}
pub fn human(w: &Workspace, actor: &str) -> Result<()> {
    ensure!(
        w.members.get(actor).is_some_and(|r| r == "human"),
        "a human member is required"
    );
    Ok(())
}
pub fn revision<'a>(
    w: &'a mut Workspace,
    req: &str,
    n: u32,
    current: bool,
) -> Result<&'a mut Revision> {
    let r = w
        .requirements
        .get_mut(req)
        .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
    ensure!(
        n > 0 && (!current || r.revisions.last().is_some_and(|r| r.number == n)),
        "stale revision; read the card again"
    );
    r.revisions
        .iter_mut()
        .find(|r| r.number == n)
        .ok_or_else(|| anyhow::anyhow!("unknown revision"))
}
pub fn apply(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    if unchanged_save(w, actor, command, position, recorded_at)? {
        return Ok(());
    }
    w.claim_time_ms = crate::tasks::parse_time(recorded_at);
    w.replay_claims = false;
    ensure!(
        !matches!(command, Command::HeartbeatTask { .. }),
        "heartbeat_task is no longer supported"
    );
    crate::objects::validate_current(w, command)?;
    match command {
        Command::ClaimTask { req, id, .. } => {
            let key = crate::tasks::address(req, id);
            if crate::tasks::task(w, &key).is_some_and(|t| crate::tasks::holder(w, t).is_none()) {
                crate::tasks::ensure_agent_available(w, actor, &key)?;
            }
        }
        Command::Task {
            req,
            assignee: Some(assignee),
            ..
        } => {
            // The assigned Task may not exist yet; its REQ is named by the command.
            let req = crate::objects::requirement_address(req);
            crate::tasks::ensure_agent_available(w, assignee, &req)?;
        }
        _ => {}
    }
    apply_inner(w, actor, command, position, recorded_at)?;
    crate::objects::sync(w, actor, command, position, recorded_at);
    crate::objects::reset_changed(w, position, recorded_at)
}

/// A live save_object that changes nothing succeeds without a revision or journal event.
pub fn unchanged_save(
    w: &Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<bool> {
    if !matches!(command, Command::SaveObject { .. }) {
        return Ok(false);
    }
    crate::objects::validate_current(w, command)?;
    identifier(actor)?;
    ensure!(w.members.contains_key(actor), "not a workspace member");
    crate::objects::unchanged_save(w, actor, command, position, recorded_at)
}

pub fn apply_replay(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    w.claim_time_ms = crate::tasks::parse_time(recorded_at);
    w.replay_claims = true;
    let result = apply_inner(w, actor, command, position, recorded_at);
    w.replay_claims = false;
    result?;
    crate::objects::sync(w, actor, command, position, recorded_at);
    crate::objects::reset_changed(w, position, recorded_at)
}

pub(crate) fn apply_inner(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    identifier(actor)?;
    if let Command::EnrollMember { id } = command {
        ensure!(
            actor == "joppa-access" && !w.admin.is_empty(),
            "internal enrollment required"
        );
        identifier(id)?;
        ensure!(
            id != "joppa-access" && !w.members.contains_key(id),
            "invalid enrollment identity"
        );
        w.members.insert(id.clone(), "human".into());
        return Ok(());
    }
    if let Command::Workspace { title } = command {
        ensure!(w.admin.is_empty(), "workspace exists");
        nonempty(title)?;
        w.title = title.clone();
        w.admin = actor.into();
        w.members.insert(actor.into(), "human".into());
        for (kind, fields) in [
            ("bug", vec!["observed", "expected", "where"]),
            ("policy", vec!["rule", "authority", "applies_to"]),
            ("question", vec!["unknown", "decides"]),
            ("idea", vec!["outcome", "today"]),
        ] {
            w.questions
                .insert(kind.into(), fields.into_iter().map(String::from).collect());
        }
        return Ok(());
    }
    ensure!(w.members.contains_key(actor), "not a workspace member");
    if crate::planning::apply(w, actor, command, position, recorded_at)? {
        return Ok(());
    }
    if crate::tasks::apply(w, actor, command, position, recorded_at)? {
        return Ok(());
    }
    match command {
        Command::MigratePersistentClaims {} => {
            ensure!(actor == w.admin, "workspace owner required");
            w.persistent_claims = true;
        }
        Command::MigrateNotes {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::objects::migrate_notes(w, actor, position, recorded_at)?;
        }
        Command::MigrateTasks {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::tasks::migrate(w, actor, position, recorded_at)?;
        }
        Command::MigrateSchema {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::objects::migrate_schema(w, actor, position, recorded_at)?;
        }
        Command::MigrateTaskClaims {} => {
            ensure!(actor == w.admin, "workspace owner required");
            if !w.claims_migrated {
                let now = w.claim_time_ms.context("server claim time is missing")?;
                for task in w.task_process.values_mut() {
                    if task.assignee.is_some() && task.done.is_none() && task.retired.is_none() {
                        task.claim = Some(crate::model::TaskClaim {
                            heartbeat_at_ms: now,
                            deadline_ms: now + crate::tasks::CLAIM_DURATION_MS,
                        });
                    }
                }
            }
            w.claims_migrated = true;
        }
        Command::HeartbeatTask { .. } => anyhow::bail!("unknown Task"),
        Command::MigrateDocuments {} => {
            ensure!(actor == w.admin, "workspace owner required");
            crate::objects::migrate_documents(w, actor, position, recorded_at)?;
        }
        Command::SetTreeSchema {
            revision,
            types,
            reason,
        } => {
            crate::objects::set_schema(w, actor, *revision, types, reason, position, recorded_at)?;
        }
        Command::SaveObject { .. } => {
            crate::objects::save(w, actor, command, position, recorded_at)?;
        }
        Command::ArchiveObject { .. } => {
            crate::objects::archive(w, actor, command, position, recorded_at)?;
        }
        Command::RestoreObject { .. } => {
            crate::objects::restore(w, actor, command, position, recorded_at)?;
        }
        Command::SetAcTests {
            target,
            tests,
            revision,
            reason,
        } => crate::ac_tests::set(
            w,
            actor,
            target,
            tests,
            *revision,
            reason,
            position,
            recorded_at,
        )?,
        Command::DecideNote {
            target,
            revision,
            outcome,
            reason,
            links,
            duplicate_of,
            question,
            recipient,
        } => {
            let decision = crate::inbox::Decision {
                target,
                revision: *revision,
                outcome,
                reason,
                links,
                duplicate_of: duplicate_of.as_deref(),
                question: question.as_deref(),
                recipient: recipient.as_deref(),
            };
            crate::inbox::decide(w, actor, &decision, position, recorded_at)?;
        }
        Command::AcceptCommit {
            target,
            revision,
            commit,
        } => crate::acceptance::accept(
            w,
            actor,
            target,
            *revision,
            commit.as_deref(),
            position,
            recorded_at,
        )?,
        Command::RedactObject {
            target,
            fragment,
            reason,
        } => crate::redact::redact(w, actor, target, fragment, reason, position, recorded_at)?,
        Command::SetNoteStatus {
            target,
            revision,
            status,
            reason,
        } => {
            let key = crate::objects::resolve_address(w, target);
            let note = w
                .objects
                .get(key)
                .ok_or_else(|| anyhow::anyhow!("unknown Note"))?;
            ensure!(
                crate::objects::input_type(w, &note.kind),
                "set_note_status requires an inbox type"
            );
            ensure!(
                ["unprocessed", "processed"].contains(&status.as_str()),
                "unknown Note status"
            );
            let r = note.current();
            let mut data = r.data.clone();
            data["status"] = serde_json::json!(status);
            let command = Command::SaveObject {
                question_checked: None,
                kind: note.kind.clone(),
                id: note.id.clone(),
                revision: *revision,
                parent: r.parent.clone(),
                data,
                links: r.links.clone(),
                reason: reason.clone(),
            };
            crate::objects::save(w, actor, &command, position, recorded_at)?;
        }
        Command::RenameWorkspace {
            workspace_id,
            title,
            reason,
            reference_prefix,
        } => {
            ensure!(actor == w.admin, "workspace owner renames the workspace");
            nonempty(title)?;
            nonempty(reason)?;
            w.title = title.clone();
            if let Some(id) = workspace_id {
                identifier(id)?;
                w.workspace_id = Some(id.clone());
            }
            if let Some(prefix) = reference_prefix {
                identifier(prefix)?;
                w.reference_prefix = Some(prefix.clone());
            }
        }
        Command::ReorderTree {
            kind,
            parent,
            expected,
            ids,
        } => {
            crate::tree_order::reorder(w, actor, kind, parent.as_deref(), expected, ids)?;
        }
        Command::Member { id, role } => {
            ensure!(actor == w.admin, "workspace owner required");
            identifier(id)?;
            ensure!(
                ["human", "agent", "runner"].contains(&role.as_str()),
                "unknown role"
            );
            ensure!(
                !w.members.contains_key(id),
                "member exists; identities cannot be reassigned"
            );
            w.members.insert(id.clone(), role.clone());
        }
        Command::Consumer {
            id,
            title,
            description,
            owner,
        } => {
            identifier(id)?;
            nonempty(title)?;
            human(w, owner)?;
            ensure!(!w.consumers.contains_key(id), "consumer exists");
            w.consumers.insert(
                id.clone(),
                Consumer {
                    title: title.clone(),
                    description: description.clone(),
                    owner: owner.clone(),
                },
            );
        }
        Command::EditConsumer {
            id,
            title,
            description,
            owner,
            reason,
        } => {
            nonempty(title)?;
            nonempty(reason)?;
            human(w, owner)?;
            let consumer = w
                .consumers
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown consumer"))?;
            ensure!(
                actor == w.admin || actor == consumer.owner,
                "consumer owner required"
            );
            consumer.title = title.clone();
            consumer.description = description.clone();
            consumer.owner = owner.clone();
        }
        Command::Domain {
            id,
            title,
            owner,
            description,
        } => {
            identifier(id)?;
            nonempty(title)?;
            human(w, owner)?;
            ensure!(!w.domains.contains_key(id), "domain exists");
            w.domains.insert(
                id.clone(),
                Domain {
                    archived: false,
                    title: title.clone(),
                    owner: owner.clone(),
                    description: description.clone(),
                    confirmed: actor == w.admin,
                },
            );
        }
        Command::Capability {
            id,
            title,
            domain,
            owner,
            description,
        } => {
            identifier(id)?;
            nonempty(title)?;
            if let Some(domain) = domain {
                ensure!(w.domains.contains_key(domain), "unknown domain");
                ensure!(!w.domains[domain].archived, "domain is archived");
            }
            ensure!(!w.capabilities.contains_key(id), "capability exists");
            let assigned = owner.as_deref().unwrap_or_else(|| {
                domain
                    .as_ref()
                    .and_then(|id| w.domains.get(id))
                    .map_or(w.admin.as_str(), |d| d.owner.as_str())
            });
            human(w, assigned)?;
            w.capabilities.insert(
                id.clone(),
                Capability {
                    title: title.clone(),
                    domain: domain.clone(),
                    owner: Some(assigned.to_owned()),
                    description: description.clone(),
                    confirmed: actor == w.admin,
                },
            );
        }
        Command::SetDomainArchived {
            id,
            archived,
            reason,
        } => {
            ensure!(actor == w.admin, "workspace owner archives domains");
            nonempty(reason)?;
            let domain = w
                .domains
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown domain"))?;
            ensure!(domain.archived != *archived, "archive state is unchanged");
            if *archived {
                ensure!(
                    !w.requirements.values().any(|r| r
                        .capability
                        .as_ref()
                        .and_then(|cap| w.capabilities.get(cap))
                        .is_some_and(|c| c.domain.as_deref() == Some(id))),
                    "only domains without requirements can be archived"
                );
            }
            w.domains.get_mut(id).unwrap().archived = *archived;
        }
        Command::ConfirmTree {} => {
            ensure!(actor == w.admin, "workspace owner confirms the tree");
            for d in w.domains.values_mut() {
                d.confirmed = true;
            }
            for c in w.capabilities.values_mut() {
                c.confirmed = true;
            }
        }
        Command::EditDomain {
            id,
            title,
            owner,
            description,
            reason,
        } => {
            let domain = w
                .domains
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown domain"))?;
            ensure!(
                actor == w.admin
                    || (w.members[actor] == "agent"
                        && !domain.confirmed
                        && !domain.archived
                        && domain.owner == *owner),
                "only the workspace owner edits confirmed domains or changes authority; agents may edit draft titles"
            );
            nonempty(title)?;
            nonempty(reason)?;
            human(w, owner)?;
            let d = w
                .domains
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown domain"))?;
            d.title = title.clone();
            d.owner = owner.clone();
            d.description = description.clone();
        }
        Command::EditCapability {
            id,
            title,
            owner,
            description,
            reason,
        } => {
            let capability = w
                .capabilities
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("unknown capability"))?;
            ensure!(
                actor == w.admin
                    || (w.members[actor] == "agent"
                        && !capability.confirmed
                        && capability
                            .domain
                            .as_ref()
                            .and_then(|id| w.domains.get(id))
                            .is_none_or(|d| !d.archived)
                        && owner
                            .as_ref()
                            .is_none_or(|requested| capability.owner.as_ref() == Some(requested))),
                "only the workspace owner edits confirmed capabilities; agents may edit draft titles"
            );
            nonempty(title)?;
            nonempty(reason)?;
            if let Some(owner) = owner {
                human(w, owner)?;
            }
            let cap = w
                .capabilities
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown capability"))?;
            cap.title = title.clone();
            if let Some(owner) = owner {
                cap.owner = Some(owner.clone());
            }
            cap.description = description.clone();
        }
        Command::WithdrawDecision {
            req,
            revision: n,
            comment,
            reason,
        } => {
            nonempty(reason)?;
            let q = w
                .requirements
                .get_mut(req)
                .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
            let c = q
                .comments
                .get_mut(comment)
                .ok_or_else(|| anyhow::anyhow!("unknown decision"))?;
            ensure!(
                c.kind == "decision" && c.actor == actor && c.revision == *n && !c.withdrawn,
                "only author may withdraw their active decision"
            );
            c.withdrawn = true;
            for r in &mut q.revisions {
                for a in &mut r.acs {
                    if a.assessment
                        .as_ref()
                        .is_some_and(|s| s.decision.as_ref() == Some(comment))
                    {
                        a.assessment = Some(Assessment {
                            ready: false,
                            reason: reason.clone(),
                            owner: Some(actor.into()),
                            decision: Some(comment.clone()),
                        });
                        r.acceptance = None;
                    }
                }
            }
        }
        Command::Project { id, repository } => {
            identifier(id)?;
            nonempty(repository)?;
            ensure!(!w.projects.contains_key(id), "project exists");
            w.projects.insert(id.clone(), repository.clone());
        }
        Command::Revise {
            req,
            expected_revision,
            capability,
            consumers,
            title,
            body,
            owner,
            reason,
            acs,
            coverage_policy,
            carry_tasks,
        } => {
            verification_identifier(w, req, "requirement")?;
            nonempty(title)?;
            nonempty(reason)?;
            human(w, owner)?;
            ensure!(
                capability
                    .as_ref()
                    .is_none_or(|id| w.capabilities.contains_key(id)),
                "unknown capability"
            );
            ensure!(
                capability
                    .as_ref()
                    .and_then(|id| w.capabilities.get(id))
                    .and_then(|c| c.domain.as_ref().and_then(|id| w.domains.get(id)))
                    .is_none_or(|d| !d.archived),
                "domain is archived"
            );
            ensure!(
                consumers.iter().collect::<BTreeSet<_>>().len() == consumers.len(),
                "duplicate consumer"
            );
            if !w.schema_migrated {
                for consumer in consumers {
                    ensure!(w.consumers.contains_key(consumer), "unknown consumer");
                }
            }
            validate_acs(w, acs)?;
            for (other_id, other) in &w.requirements {
                if other_id != req {
                    ensure!(
                        !other
                            .revisions
                            .iter()
                            .any(|r| r.acs.iter().any(|old| acs.iter().any(|a| a.id == old.id))),
                        "AC id belongs to another REQ"
                    );
                }
            }
            let r = w
                .requirements
                .entry(req.clone())
                .or_insert_with(|| Requirement {
                    capability: capability.clone(),
                    revisions: vec![],
                    comments: BTreeMap::new(),
                });
            ensure!(
                r.revisions.last().map_or(0, |r| r.number) == *expected_revision,
                "stale revision; read the card again"
            );
            if let Some(old) = r.revisions.last() {
                ensure!(
                    old.owner == *owner || actor == old.owner,
                    "only current owner can transfer responsibility"
                );
            }
            r.capability = capability.clone();
            let sources = r
                .revisions
                .last()
                .map(|old| old.sources.clone())
                .unwrap_or_default();
            let prior_checks = r
                .revisions
                .last()
                .map(|old| {
                    old.runs
                        .values()
                        .max_by_key(|run| run.started)
                        .map(|run| run.plan.clone())
                        .unwrap_or_else(|| old.prior_checks.clone())
                })
                .unwrap_or_default();
            let mut tasks = BTreeMap::new();
            if *carry_tasks {
                for old in &r.revisions {
                    for (id, task) in &old.tasks {
                        let mut task = task.clone();
                        task.review_required = task.retired.is_none();
                        if task.origin_revision == 0 {
                            task.origin_revision = old.number;
                        }
                        tasks.insert(id.clone(), task);
                    }
                }
            }
            let acs = acs
                .iter()
                .cloned()
                .map(|mut ac| {
                    ac.assessment = None;
                    ac
                })
                .collect();
            r.revisions.push(Revision {
                number: expected_revision + 1,
                capability: capability.clone(),
                consumers: consumers.clone(),
                title: title.clone(),
                body: body.clone(),
                owner: owner.clone(),
                reason: reason.clone(),
                acs,
                confirmed: false,
                tasks,
                runs: BTreeMap::new(),
                issues: BTreeMap::new(),
                acceptance: None,
                coverage_policy: *coverage_policy,
                coverage_change: Vec::new(),
                prior_checks,
                coverage_review: None,
                verification_scope: None,
                sources,
            });
        }
        Command::LinkSource {
            req,
            revision: n,
            id,
            kind,
            uri,
            external_id,
            version,
            relation,
        } => {
            identifier(id)?;
            identifier(kind)?;
            ensure!(
                !uri.trim().is_empty()
                    || external_id.as_ref().is_some_and(|id| !id.trim().is_empty()),
                "source needs a URI or an external identifier"
            );
            nonempty(version)?;
            ensure!(
                ["origin", "supports", "contradicts", "decision"].contains(&relation.as_str()),
                "unknown source relation"
            );
            let r = revision(w, req, *n, true)?;
            ensure!(!r.sources.contains_key(id), "source reference exists");
            r.sources.insert(
                id.clone(),
                SourceRef {
                    kind: kind.clone(),
                    uri: uri.clone(),
                    external_id: external_id.clone(),
                    version: version.clone(),
                    relation: relation.clone(),
                    actor: actor.to_owned(),
                    at: recorded_at.to_owned(),
                    withdrawn: false,
                },
            );
        }
        Command::WithdrawSource {
            req,
            revision: n,
            id,
            reason,
        } => {
            nonempty(reason)?;
            let r = revision(w, req, *n, true)?;
            let source = r
                .sources
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown source reference"))?;
            ensure!(!source.withdrawn, "source reference already withdrawn");
            ensure!(
                actor == source.actor || actor == r.owner,
                "source author or REQ owner required"
            );
            source.withdrawn = true;
        }
        Command::Confirm { req, revision: n } => {
            let q = w
                .requirements
                .get(req)
                .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
            ensure!(
                q.capability
                    .as_ref()
                    .and_then(|id| w.capabilities.get(id))
                    .is_none_or(|c| c.confirmed
                        && c.domain
                            .as_ref()
                            .and_then(|id| w.domains.get(id))
                            .is_none_or(|d| d.confirmed)),
                "workspace owner must confirm the business tree first"
            );
            let r = revision(w, req, *n, true)?;
            ensure!(r.owner == actor, "REQ owner confirms its formulation");
            r.confirmed = true;
        }
        Command::Describe {
            target: t,
            text,
            reason,
        } => {
            target(w, t)?;
            nonempty(reason)?;
            ensure!(
                text.chars().count() <= DESCRIPTION_MAX,
                "a description is at most {DESCRIPTION_MAX} characters"
            );
            // Old journal events keep embedded descriptions. Current objects use common annotations.
            if !w.documents_migrated
                && let Some(domain) = t
                    .strip_prefix("domain:")
                    .and_then(|id| w.domains.get_mut(id))
            {
                domain.description = text.clone();
            } else if !w.documents_migrated
                && let Some(capability) = t
                    .strip_prefix("capability:")
                    .and_then(|id| w.capabilities.get_mut(id))
            {
                capability.description = text.clone();
            } else {
                w.notes
                    .entry(crate::objects::annotation_address(w, t))
                    .or_default()
                    .description = text.clone();
            }
        }
        Command::Note {
            target: t,
            id,
            text,
            reply_to,
        } => {
            target(w, t)?;
            identifier(id)?;
            nonempty(text)?;
            let notes = w
                .notes
                .entry(crate::objects::annotation_address(w, t))
                .or_default();
            ensure!(!notes.comments.contains_key(id), "comment {id} exists");
            if let Some(parent) = reply_to {
                ensure!(
                    notes.comments.contains_key(parent),
                    "no such comment to reply to: {parent}"
                );
            }
            notes.comments.insert(
                id.clone(),
                Note {
                    actor: actor.into(),
                    at: recorded_at.to_string(),
                    text: text.clone(),
                    reply_to: reply_to.clone(),
                    withdrawn: false,
                },
            );
        }
        Command::WithdrawNote {
            target: t,
            id,
            reason,
        } => {
            target(w, t)?;
            nonempty(reason)?;
            let annotation = crate::objects::annotation_address(w, t);
            let note = w
                .notes
                .get_mut(&annotation)
                .and_then(|n| n.comments.get_mut(id))
                .ok_or_else(|| anyhow::anyhow!("no such comment: {id}"))?;
            ensure!(note.actor == actor, "a comment is withdrawn by its author");
            note.withdrawn = true;
        }
        Command::Attach {
            target: t,
            id,
            name,
            content_type,
            size,
            hash,
        } => {
            target(w, t)?;
            identifier(id)?;
            nonempty(name)?;
            ensure!(*size > 0, "an empty attachment carries nothing");
            // Именно НИЖНИЙ регистр: хранилище кладёт файл под ключом из
            // {:x}, и тот же отпечаток заглавными буквами прошёл бы проверку,
            // но не нашёл бы файла — вложение было бы записано и не
            // открывалось бы.
            ensure!(
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "the attachment digest is a sha256 in lowercase hex"
            );
            let notes = w
                .notes
                .entry(crate::objects::annotation_address(w, t))
                .or_default();
            ensure!(
                !notes.attachments.contains_key(id),
                "attachment {id} exists"
            );
            notes.attachments.insert(
                id.clone(),
                Attachment {
                    name: name.clone(),
                    content_type: if content_type.is_empty() {
                        "application/octet-stream".into()
                    } else {
                        content_type.clone()
                    },
                    size: *size,
                    hash: hash.clone(),
                    actor: actor.into(),
                    at: recorded_at.to_string(),
                    removed: false,
                },
            );
        }
        Command::Detach {
            target: t,
            id,
            reason,
        } => {
            target(w, t)?;
            nonempty(reason)?;
            let annotation = crate::objects::annotation_address(w, t);
            let a = w
                .notes
                .get_mut(&annotation)
                .and_then(|n| n.attachments.get_mut(id))
                .ok_or_else(|| anyhow::anyhow!("no such attachment: {id}"))?;
            a.removed = true;
        }
        Command::Comment {
            req,
            revision: n,
            id,
            text,
            kind,
            reply_to,
            ac,
        } => {
            identifier(id)?;
            nonempty(text)?;
            ensure!(
                [
                    "comment",
                    "decision",
                    "accept",
                    "change",
                    "reject",
                    "correction"
                ]
                .contains(&kind.as_str()),
                "unknown comment kind"
            );
            if ["decision", "accept", "reject", "change"].contains(&kind.as_str()) {
                person(w, actor)?;
            }
            let r = revision(w, req, *n, false)?;
            if ["accept", "change", "reject"].contains(&kind.as_str()) {
                ensure!(r.owner == actor, "owner replies to suggestions");
            }
            if let Some(a) = ac {
                ensure!(r.acs.iter().any(|c| &c.id == a), "unknown AC");
            }
            let q = w.requirements.get_mut(req).unwrap();
            ensure!(!q.comments.contains_key(id), "comment id exists");
            if let Some(p) = reply_to {
                ensure!(q.comments.contains_key(p), "unknown previous comment");
            }
            if kind == "correction" {
                ensure!(
                    reply_to.is_some(),
                    "correction must reference prior comment"
                );
            }
            q.comments.insert(
                id.clone(),
                Comment {
                    revision: *n,
                    actor: actor.into(),
                    // Из журнала, не от клиента: по этим отметкам потом
                    // восстанавливают, кто что знал и когда.
                    at: recorded_at.to_string(),
                    text: text.clone(),
                    kind: kind.clone(),
                    reply_to: reply_to.clone(),
                    ac: ac.clone(),
                    withdrawn: false,
                },
            );
        }
        Command::Assess {
            req,
            revision: n,
            ac,
            assessment,
        } => {
            nonempty(&assessment.reason)?;
            let q = w
                .requirements
                .get(req)
                .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
            let r = q.revisions.last().unwrap();
            ensure!(r.number == *n, "stale revision");
            let a = r
                .acs
                .iter()
                .find(|a| &a.id == ac)
                .ok_or_else(|| anyhow::anyhow!("unknown AC"))?;
            if !assessment.ready {
                let recipient = assessment
                    .owner
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("waiting needs a recipient"))?;
                ensure!(w.members.contains_key(recipient), "unknown recipient");
            }
            if assessment.ready && a.owner.is_some() {
                let decision = assessment
                    .decision
                    .as_ref()
                    .and_then(|id| q.comments.get(id))
                    .ok_or_else(|| anyhow::anyhow!("specialist decision must be linked"))?;
                let same_formulation = q
                    .revisions
                    .iter()
                    .find(|source| source.number == decision.revision)
                    .is_some_and(|source| {
                        source.title == r.title
                            && source.body == r.body
                            && source.acs.iter().any(|old| {
                                old.id == a.id
                                    && old.text == a.text
                                    && old.owner == a.owner
                                    && old.depends_on == a.depends_on
                                    && old.before_launch == a.before_launch
                            })
                    });
                ensure!(
                    decision.kind == "decision"
                        && !decision.withdrawn
                        && Some(&decision.actor) == a.owner.as_ref()
                        && decision.ac.as_ref() == Some(ac)
                        && (decision.revision == *n || same_formulation),
                    "decision must be by designated human for this AC and unchanged formulation"
                );
            } else if let Some(id) = &assessment.decision {
                ensure!(
                    q.comments
                        .get(id)
                        .is_some_and(|c| c.kind == "decision" && !c.withdrawn),
                    "unknown or withdrawn decision reference"
                );
            }
            let r = revision(w, req, *n, true)?;
            r.acs.iter_mut().find(|a| &a.id == ac).unwrap().assessment = Some(assessment.clone());
            r.acceptance = None;
        }
        Command::Workspace { .. } => unreachable!(),
        _ => checks::apply(w, actor, command, position, recorded_at)?,
    }
    Ok(())
}
fn validate_acs(w: &Workspace, acs: &[Ac]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for ac in acs {
        verification_identifier(w, &ac.id, "ac")?;
        nonempty(&ac.text)?;
        ensure!(ids.insert(&ac.id), "duplicate AC id");
        ensure!(
            ac.assessment.is_none(),
            "assess each AC after saving its revision"
        );
        if let Some(d) = &ac.owner {
            human(w, d)?;
        }
    }
    fn visit<'a>(
        id: &'a str,
        acs: &'a [Ac],
        stack: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        ensure!(stack.insert(id), "AC dependency cycle");
        let a = acs
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown AC dependency"))?;
        for dep in &a.depends_on {
            visit(dep, acs, stack, done)?;
        }
        stack.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for a in acs {
        visit(&a.id, acs, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

pub fn ready(r: &Revision, id: &str) -> bool {
    let Some(ac) = r.acs.iter().find(|a| a.id == id) else {
        return false;
    };
    ac.assessment.as_ref().is_some_and(|a| a.ready) && ac.depends_on.iter().all(|d| ready(r, d))
}

pub fn unknown() -> Result<()> {
    bail!("unsupported operation")
}
