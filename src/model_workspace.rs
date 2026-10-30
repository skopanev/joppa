//! Part of `model`: `workspace` and the items after it, moved from model.rs unchanged.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Workspace {
    #[serde(default)]
    pub persistent_claims: bool,
    #[serde(skip)]
    pub replay_claims: bool,
    #[serde(skip)]
    pub claim_time_ms: Option<i64>,
    /// The first journal position of each replay rule (joppa.replay_epochs). A replay
    /// gives older events the meaning that they had when they were written.
    #[serde(skip)]
    pub replay_epochs: BTreeMap<String, u64>,
    #[serde(default)]
    pub claims_migrated: bool,
    #[serde(skip)]
    pub unloaded_objects: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub notes_migrated: bool,
    #[serde(default)]
    pub object_aliases: BTreeMap<String, String>,
    #[serde(default)]
    pub objects: BTreeMap<String, crate::objects::Object>,
    /// Requirement acceptances per object, each on one revision and one commit.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub acceptances: BTreeMap<String, Vec<crate::acceptance::Acceptance>>,
    /// Owner redactions per object. The fragment itself stays only in the journal.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub redactions: BTreeMap<String, Vec<crate::redact::Redaction>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub task_process: BTreeMap<String, Task>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub planning_claims: BTreeMap<String, PlanningClaim>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub task_context: BTreeMap<String, Option<(String, u32)>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tasks_migrated: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub schema_migrated: bool,
    /// The migrate_consumers event ran: each criterion names one consumer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub consumers_migrated: bool,
    /// The migrate_consumer_drops event ran: each criterion that lost consumers has a comment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub consumer_drops_noted: bool,
    /// The migrate_test_rules event ran: the schema holds every test rule and status rule.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub test_rules_migrated: bool,
    /// The migrate_note_parents event ran: inbox objects have no parent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub note_parents_migrated: bool,
    /// The reference prefix comes from the title, so a rename derives it again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prefix_from_title: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub documents_migrated: bool,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub document_overrides: std::collections::BTreeSet<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub archived_objects: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub tree_schema: Vec<crate::objects::SchemaRevision>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub reference_prefix: Option<String>,
    pub admin: String,
    pub members: BTreeMap<String, String>,
    /// The readiness of each signal of an inbox type, marked by a member.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub signal_assessments: BTreeMap<String, SignalAssessment>,
    /// The priority value of each node that has one. It is not a change of the node.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub priorities: BTreeMap<String, String>,
    /// The journal position of the owner's deletion. A deleted workspace has no members.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<u64>,
    pub domains: BTreeMap<String, Domain>,
    #[serde(default)]
    pub tree_order: BTreeMap<String, Vec<String>>,
    pub capabilities: BTreeMap<String, Capability>,
    #[serde(default)]
    pub consumers: BTreeMap<String, Consumer>,
    pub projects: BTreeMap<String, String>,
    pub requirements: BTreeMap<String, Requirement>,
    pub intake: BTreeMap<String, Intake>,
    pub questions: BTreeMap<String, Vec<String>>,
    /// Пояснения, обсуждение и вложения — на ЛЮБОМ объекте, по его адресу.
    ///
    /// Ключ — адрес вида `domain:billing`, `capability:billing.invoice`,
    /// `requirement:r-xxx`, `ac:r-xxx/2`, `intake:i-xxx`, `point:i-xxx/1`.
    /// Отдельная карта, а не три поля в каждой структуре: иначе один и тот
    /// же механизм пришлось бы написать шесть раз и он разошёлся бы
    /// шестью способами.
    #[serde(default)]
    pub notes: BTreeMap<String, Notes>,
    /// Tests that prove each criterion, by criterion address, with their own history.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ac_tests: BTreeMap<String, crate::ac_tests::AcTests>,
    /// The journal time of the latest change above or on each criterion.
    /// Reports received at or before it do not count for the criterion.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub criterion_boundaries: BTreeMap<String, String>,
    /// The links of each object to objects of other workspaces, by source address.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub remote_links: BTreeMap<String, crate::remote_links::RemoteLinks>,
}

/// Всё, что человек навесил на объект сверх его собственных полей.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Notes {
    /// Пояснение к объекту. Для домена, капабилити и AC описание живёт в
    /// самом объекте — оно версионируется вместе с ним; сюда попадает
    /// описание тех объектов, у которых своего поля нет.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub comments: BTreeMap<String, Note>,
    #[serde(default)]
    pub attachments: BTreeMap<String, Attachment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub actor: String,
    /// Время записи в журнал, а не присланное клиентом.
    pub at: String,
    pub text: String,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub withdrawn: bool,
}

/// Вложение: здесь только про файл, самого файла здесь нет.
///
/// Байты лежат в Spaces под files/<отпечаток> — см. src/spaces.rs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub hash: String,
    pub actor: String,
    pub at: String,
    #[serde(default)]
    pub removed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Domain {
    #[serde(default)]
    pub archived: bool,
    pub title: String,
    #[serde(alias = "authority")]
    pub owner: String,
    /// Свободное пояснение: что сюда относится и чем этот домен не является.
    /// Заголовок в шестьдесят знаков не вмещает границу, а спор о границе —
    /// самый частый спор о дереве.
    #[serde(default)]
    pub description: String,
    pub confirmed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capability {
    pub title: String,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub description: String,
    pub confirmed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Consumer {
    pub title: String,
    pub description: String,
    pub owner: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Requirement {
    #[serde(default)]
    pub capability: Option<String>,
    pub revisions: Vec<Revision>,
    pub comments: BTreeMap<String, Comment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Revision {
    pub number: u32,
    #[serde(default)]
    pub capability: Option<String>,
    #[serde(default)]
    pub consumers: Vec<String>,
    pub title: String,
    pub body: String,
    pub owner: String,
    pub reason: String,
    pub acs: Vec<Ac>,
    pub confirmed: bool,
    pub tasks: BTreeMap<String, Task>,
    pub runs: BTreeMap<String, Run>,
    pub issues: BTreeMap<String, Issue>,
    pub acceptance: Option<Acceptance>,
    #[serde(default)]
    pub coverage_policy: bool,
    #[serde(default)]
    pub coverage_change: Vec<Check>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prior_checks: Vec<Check>,
    #[serde(default)]
    pub coverage_review: Option<CoverageReview>,
    /// The owner-approved component set and check boundary for this revision.
    #[serde(default)]
    pub verification_scope: Option<VerificationScope>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoverageReview {
    pub actor: String,
    pub at: u64,
    pub reason: String,
    pub changes: Vec<Check>,
}
