use crate::{checks, model::*};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Internal migration event. Public authoring rejects this operation.
    MigrateNotes {},
    MigrateTasks {},
    MigrateSchema {},
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

pub fn nonempty(s: &str) -> Result<()> {
    ensure!(!s.trim().is_empty(), "text must not be empty");
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
        n > 0 && (!current || n as usize == r.revisions.len()),
        "stale revision; read the card again"
    );
    r.revisions
        .get_mut((n - 1) as usize)
        .ok_or_else(|| anyhow::anyhow!("unknown revision"))
}
pub fn apply(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    crate::objects::validate_current(w, command)?;
    apply_replay(w, actor, command, position, recorded_at)
}

pub fn apply_replay(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    recorded_at: &str,
) -> Result<()> {
    apply_inner(w, actor, command, position, recorded_at)?;
    crate::objects::sync(w, actor, command, position, recorded_at);
    Ok(())
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
    if crate::tasks::apply(w, actor, command, position, recorded_at)? {
        return Ok(());
    }
    match command {
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
            identifier(req)?;
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
                r.revisions.len() == *expected_revision as usize,
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
            nonempty(uri)?;
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
            // Домен и капабилити НЕ версионируются — их пояснение живёт в
            // них самих, и правка здесь ничего не переписывает задним числом.
            // У условия приёмки описание — часть ревизии, а ревизию трогать
            // нельзя: там оно остаётся как было, а это пояснение ложится
            // рядом, отдельной заметкой.
            if let Some(id) = t.strip_prefix("domain:") {
                w.domains.get_mut(id).unwrap().description = text.clone();
            } else if let Some(id) = t.strip_prefix("capability:") {
                w.capabilities.get_mut(id).unwrap().description = text.clone();
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
            if kind == "decision" {
                human(w, actor)?;
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
                    .get((decision.revision - 1) as usize)
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
        identifier(&ac.id)?;
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
