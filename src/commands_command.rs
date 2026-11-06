//! Part of `commands`: `command` and the items after it, moved from commands.rs unchanged.
use super::*;

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
    /// Internal migration event: one consumer per criterion, see consumer_migration.
    MigrateConsumers {},
    /// Internal migration event: a comment on each criterion that lost consumers, see consumer_drops.
    MigrateConsumerDrops {},
    /// Internal migration event: the schema gets every test rule, see test_rules.
    MigrateTestRules {},
    MigrateNoteParents {},
    /// Internal migration event: the text of every object type is body, see body_migration.
    MigrateBody {},
    /// Internal migration event: a Task has no purpose, see purpose_migration.
    MigratePurpose {},
    MigrateTestNodes {},
    /// Internal migration event: a Task is a node like any other, see generic_nodes.
    MigrateGenericNodes {},
    /// Internal migration event: a node has no decision_required, see decision_migration.
    MigrateDecisionRequired {},
    SetTreeSchema(SetTreeSchemaOp),
    /// The older readiness mark of an inbox signal. Only journal replay reads it; see `assess`.
    AssessSignal {
        target: String,
        ready: bool,
        reason: String,
    },
    /// The older priority of a node. Only journal replay reads it; a priority is `data.priority`.
    SetPriority {
        target: String,
        priority: String,
    },
    SaveObject(SaveObjectOp),
    ArchiveObject {
        target: String,
        revision: u32,
        reason: String,
    },
    /// Replace the links of one object to objects of other workspaces.
    SetRemoteLinks {
        target: String,
        /// The remote links revision that the writer read; 0 when the object has none.
        revision: u32,
        links: Vec<crate::remote_links::RemoteLink>,
        reason: String,
    },
    RestoreObject {
        target: String,
        revision: u32,
        reason: String,
    },
    /// Save the list of tests that prove a criterion. It keeps its own history.
    SetAcTests(SetAcTestsOp),
    /// Decide an Inbox signal: changed_spec, needs_information, rejected or duplicate.
    DecideNote(DecideNoteOp),
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
    SetNoteStatus(SetNoteStatusOp),
    Workspace {
        title: String,
    },
    RenameWorkspace(RenameWorkspaceOp),
    UpdateWorkspace(UpdateWorkspaceOp),
    /// The owner removes a member. The owner cannot remove themself.
    RemoveMember {
        id: String,
        reason: String,
    },
    /// The owner deletes the workspace. The write then removes every row of it from the database.
    DeleteWorkspace {
        reason: String,
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
    Consumer(ConsumerOp),
    EditConsumer(EditConsumerOp),
    /// Internal event of a workspace deletion: remove the remote links and child workspace
    /// bindings that name the deleted workspace. Public command entry points reject it.
    RemoveWorkspaceReferences {
        workspace: String,
        reason: String,
    },
    /// Internal enrollment event; public command entry points reject this operation.
    EnrollMember {
        id: String,
    },
    /// Internal enrollment of a service identity, an agent or a runner, by the workspace
    /// owner. Public command entry points reject this operation.
    EnrollService {
        id: String,
        role: String,
    },
    /// Internal event of a record of test results: it gives the record its journal position.
    /// The report itself stays in joppa.test_reports; the event changes no state.
    RecordReport(RecordReportOp),
    Domain(DomainOp),
    Capability(CapabilityOp),
    SetDomainArchived(SetDomainArchivedOp),
    ConfirmTree(ConfirmTreeOp),
    EditDomain(EditDomainOp),
    EditCapability(EditCapabilityOp),
    WithdrawDecision(WithdrawDecisionOp),
    Project(ProjectOp),
    Revise(ReviseOp),
    ReviewCoverage(ReviewCoverageOp),
    DefineVerificationScope(DefineVerificationScopeOp),
    LinkSource(LinkSourceOp),
    WithdrawSource(WithdrawSourceOp),
    Confirm {
        req: String,
        revision: u32,
    },
    Comment(CommentOp),
    /// Пояснение к объекту, у которого нет своего поля описания.
    /// У домена, капабилити и AC оно есть — там его правят вместе с самим
    /// объектом, чтобы описание менялось одной ревизией с тем, что описывает.
    Describe(DescribeOp),
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
    Attach(AttachOp),
    /// Remove an attachment. The file stays in storage for 14 days, then the periodic
    /// reconcile deletes it when no active attachment of the workspace names its hash.
    Detach {
        target: String,
        id: String,
        reason: String,
    },
    Assess(AssessOp),
    Task(TaskOp),
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
    AmendTask(AmendTaskOp),
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
    CompleteTask(CompleteTaskOp),
    ReviewTask(ReviewTaskOp),
    RetireTask {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        req: String,
        revision: u32,
        id: String,
        reason: String,
    },
    Plan(PlanOp),
    Result(ResultOp),
    Finish(FinishOp),
    Accept(AcceptOp),
    Revoke(RevokeOp),
    Issue(IssueOp),
    Resolve(ResolveOp),
    Intake(IntakeOp),
    Split(SplitOp),
    Questions(QuestionsOp),
    Sort(SortOp),
    Answer(AnswerOp),
    Route(RouteOp),
}
