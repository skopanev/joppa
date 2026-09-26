use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Workspace {
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub reference_prefix: Option<String>,
    pub admin: String,
    pub members: BTreeMap<String, String>,
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
/// Байты лежат в joppa.blobs по этому отпечатку — см. миграцию 009.
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
    pub domain: String,
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
    pub capability: String,
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationScope {
    pub subject_keys: Vec<String>,
    pub scope: String,
    pub actor: String,
    pub at: u64,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRef {
    pub kind: String,
    pub uri: String,
    pub external_id: Option<String>,
    pub version: String,
    pub relation: String,
    pub actor: String,
    pub at: String,
    #[serde(default)]
    pub withdrawn: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ac {
    pub id: String,
    pub text: String,
    /// Пояснение к условию: оговорки, границы, чего оно НЕ требует.
    /// Сам текст AC остаётся коротким — он должен читаться одним взглядом,
    /// а подробности, без которых спорят, живут здесь.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    #[serde(alias = "decider")]
    pub owner: Option<String>,
    #[serde(default)]
    pub before_launch: bool,
    #[serde(default)]
    pub assessment: Option<Assessment>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub ready: bool,
    pub reason: String,
    #[serde(alias = "recipient")]
    pub owner: Option<String>,
    pub decision: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Comment {
    pub revision: u32,
    pub actor: String,
    /// Время записи в журнал, а НЕ присланное клиентом. Второй источник
    /// правды о времени означал бы, что его можно подделать, а по этим
    /// отметкам потом восстанавливают, кто что знал и когда.
    #[serde(default)]
    pub at: String,
    pub text: String,
    pub kind: String,
    pub reply_to: Option<String>,
    pub ac: Option<String>,
    pub withdrawn: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Task {
    pub title: String,
    pub acs: Vec<String>,
    pub projects: Vec<String>,
    pub purpose: String,
    /// An unassigned task becomes in progress only after an atomic claim.
    pub assignee: Option<String>,
    pub done: Option<String>,
    #[serde(default)]
    pub contract: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub created_by: String,
    /// A copied task stays visible after REQ revision, but its fit to the new
    /// wording must be confirmed before unfinished work can be completed.
    #[serde(default)]
    pub review_required: bool,
    #[serde(default)]
    pub origin_revision: u32,
    #[serde(default)]
    pub retired: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub ac: String,
    pub method: String,
    pub expected: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub check: String,
    pub outcome: String,
    pub observed: Vec<String>,
    pub note: String,
    #[serde(default)]
    pub link: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Run {
    pub subject: BTreeMap<String, String>,
    pub plan: Vec<Check>,
    pub discovery_complete: bool,
    pub scope: String,
    pub reporter: String,
    pub started: u64,
    pub finished: Option<u64>,
    pub results: BTreeMap<String, Evidence>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub ac: String,
    pub comment: String,
    pub opened: u64,
    pub resolved: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Acceptance {
    pub run: String,
    pub actor: String,
    pub at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Intake {
    pub said: String,
    pub author: String,
    #[serde(default)]
    pub owner: Option<String>,
    pub points: BTreeMap<String, Point>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Point {
    pub said: String,
    pub kind: String,
    pub answers: BTreeMap<String, String>,
    pub outcome: Option<String>,
    pub reason: Option<String>,
    pub requirement: Option<String>,
}
