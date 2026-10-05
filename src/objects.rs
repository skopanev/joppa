//! Common document revisions derived from the immutable workspace journal.
use crate::{
    commands::{self, Command},
    model::*,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TreeType {
    pub title: String,
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub root: bool,
    pub role: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaRevision {
    pub revision: u32,
    pub types: BTreeMap<String, TreeType>,
    pub actor: String,
    pub at: String,
    pub position: u64,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObjectLink {
    pub target: String,
    pub relation: String,
    #[serde(default)]
    pub revision: Option<u32>,
    #[serde(default)]
    pub blocking: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectRevision {
    pub revision: u32,
    pub schema_revision: u32,
    pub parent: Option<String>,
    pub data: Value,
    pub links: Vec<ObjectLink>,
    pub actor: String,
    pub at: String,
    pub position: u64,
    pub reason: String,
    pub present: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Object {
    pub kind: String,
    pub id: String,
    pub revisions: Vec<ObjectRevision>,
    #[serde(default)]
    pub parent_override: bool,
}
impl Object {
    pub fn current(&self) -> &ObjectRevision {
        self.revisions.last().expect("object has a revision")
    }
}
pub fn defaults() -> BTreeMap<String, TreeType> {
    [
        ("domain", "Domain", "group", true, vec![]),
        ("capability", "Capability", "group", false, vec!["domain"]),
        (
            "requirement",
            "Requirement",
            "requirement",
            false,
            vec!["capability"],
        ),
        (
            "ac",
            "Acceptance criterion",
            "ac",
            false,
            vec!["requirement"],
        ),
        ("task", "Task", "task", false, vec!["ac"]),
        ("consumer", "Consumer", "document", true, vec![]),
        ("note", "Note", "document", true, vec!["*"]),
        ("project", "Project", "document", true, vec![]),
        ("intake", "Source note", "document", true, vec![]),
        ("point", "Source point", "document", false, vec!["intake"]),
    ]
    .into_iter()
    .map(|(kind, title, role, root, parents)| {
        (
            kind.into(),
            TreeType {
                title: title.into(),
                role: role.into(),
                root,
                parents: parents.into_iter().map(String::from).collect(),
            },
        )
    })
    .collect()
}
pub fn schema(w: &Workspace) -> SchemaRevision {
    w.tree_schema
        .last()
        .cloned()
        .unwrap_or_else(|| SchemaRevision {
            revision: 0,
            types: defaults(),
            actor: w.admin.clone(),
            at: String::new(),
            position: 0,
            reason: "Default workspace tree".into(),
        })
}
fn address(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}
fn parent_valid(
    types: &BTreeMap<String, TreeType>,
    w: &Workspace,
    kind: &str,
    parent: Option<&str>,
) -> Result<()> {
    let t = types
        .get(kind)
        .context("object type is absent from the tree schema")?;
    match parent {
        None => ensure!(t.root, "this type requires a parent"),
        Some(p) => {
            let obj = w.objects.get(p).context("unknown parent")?;
            ensure!(
                obj.current().present || kind == "task" && obj.kind == "ac",
                "parent is absent from the current tree"
            );
            ensure!(
                t.parents.iter().any(|v| v == "*" || v == &obj.kind),
                "parent type is not permitted by the tree schema"
            );
        }
    }
    Ok(())
}
pub fn set_schema(
    w: &mut Workspace,
    actor: &str,
    revision: u32,
    types: &BTreeMap<String, TreeType>,
    reason: &str,
    position: u64,
    at: &str,
) -> Result<()> {
    ensure!(
        w.members.get(actor).is_some_and(|role| role == "agent") || actor == w.admin,
        "workspace owner or agent required"
    );
    ensure!(
        schema(w).revision == revision,
        "stale schema revision; read the tree schema again"
    );
    commands::nonempty(reason)?;
    ensure!(
        !types.is_empty() && types.len() <= 100,
        "tree schema needs 1 to 100 types"
    );
    for (kind, t) in types {
        commands::identifier(kind)?;
        commands::nonempty(&t.title)?;
        ensure!(
            ["group", "document", "requirement", "ac", "task"].contains(&t.role.as_str()),
            "unknown semantic role"
        );
        if let Some(original) = defaults().get(kind) {
            ensure!(
                t.role == original.role,
                "built-in semantic roles cannot change"
            );
        } else {
            ensure!(
                ["group", "document"].contains(&t.role.as_str()),
                "REQ, AC and Task roles use their built-in types"
            );
        }
        for p in &t.parents {
            ensure!(p == "*" || types.contains_key(p), "unknown parent type");
        }
    }
    for (kind, parent) in [("ac", "requirement"), ("task", "ac")] {
        let t = types.get(kind).context("AC and Task types are required")?;
        ensure!(
            !t.root && t.parents == vec![parent.to_string()],
            "AC has one REQ parent; Task has one AC parent"
        );
    }
    ensure!(
        types.contains_key("requirement") && types.contains_key("note"),
        "REQ and Note types are required"
    );
    for obj in w.objects.values().filter(|o| o.current().present) {
        ensure!(
            types.contains_key(&obj.kind),
            "object type is absent from the tree schema"
        );
        if !legacy_task_binding(w, obj) {
            parent_valid(types, w, &obj.kind, obj.current().parent.as_deref())?;
        }
    }
    w.tree_schema.push(SchemaRevision {
        revision: revision + 1,
        types: types.clone(),
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.into(),
    });
    Ok(())
}

fn document_rows(w: &Workspace) -> BTreeMap<String, (String, Option<String>, Value)> {
    let mut rows = BTreeMap::new();
    let mut put = |kind: &str, id: &str, parent: Option<String>, data: Value| {
        rows.insert(address(kind, id), (kind.into(), parent, data));
    };
    for (id, d) in &w.domains {
        put(
            "domain",
            id,
            None,
            json!({"title":d.title,"owner":d.owner,"description":d.description,"archived":d.archived}),
        );
    }
    for (id, c) in &w.capabilities {
        put(
            "capability",
            id,
            c.domain.as_deref().map(|id| address("domain", id)),
            json!({"title":c.title,"owner":c.owner,"description":c.description}),
        );
    }
    for (id, c) in &w.consumers {
        put(
            "consumer",
            id,
            None,
            json!({"title":c.title,"owner":c.owner,"description":c.description}),
        );
    }
    for (id, p) in &w.projects {
        put(
            "project",
            id,
            None,
            json!({"title":id,"repository":p,"owner":w.admin}),
        );
    }
    for (id, q) in &w.requirements {
        if let Some(r) = q.revisions.last() {
            put(
                "requirement",
                id,
                q.capability.as_deref().map(|id| address("capability", id)),
                json!({"title":r.title,"body":r.body,"owner":r.owner,"consumers":r.consumers}),
            );
            for ac in &r.acs {
                put(
                    "ac",
                    &ac.id,
                    Some(address("requirement", id)),
                    json!({"title":ac.text,"description":ac.description,"owner":ac.owner.as_ref().unwrap_or(&r.owner),"decision_required":ac.owner.is_some(),"depends_on":ac.depends_on,"before_launch":ac.before_launch}),
                );
            }
            for (tid, t) in &r.tasks {
                let mut data = json!({"title":t.title,"owner":r.owner,"contract":t.contract,"projects":t.projects,"purpose":t.purpose});
                let parent = if t.acs.len() == 1 {
                    address("ac", &t.acs[0])
                } else {
                    // Preserve old bindings without permitting new Tasks with multiple ACs.
                    data["legacy_acs"] = json!(t.acs);
                    address("requirement", id)
                };
                put("task", &format!("{id}/{tid}"), Some(parent), data);
            }
        }
    }
    for (id, i) in &w.intake {
        put(
            "intake",
            id,
            None,
            json!({"title":id,"body":i.said,"owner":i.owner.as_ref().unwrap_or(&w.admin),"author":i.author}),
        );
        for (pid, p) in &i.points {
            put(
                "point",
                &format!("{id}/{pid}"),
                Some(address("intake", id)),
                json!({"title":p.said,"owner":i.owner.as_ref().unwrap_or(&w.admin),"kind":p.kind,"answers":p.answers,"outcome":p.outcome,"reason":p.reason,"requirement":p.requirement}),
            );
        }
    }
    rows
}
/// Replay old commands into the same document ledger. Process evidence stays in its original revision.
pub fn sync(w: &mut Workspace, actor: &str, command: &Command, position: u64, at: &str) {
    let rows = document_rows(w);
    let reason = serde_json::to_value(command)
        .ok()
        .and_then(|v| v["reason"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "Journal operation".into());
    let schema_revision = schema(w).revision;
    let forced_req = match command {
        Command::Revise { req, .. } => Some(req.as_str()),
        _ => None,
    };
    for (key, (kind, parent, data)) in &rows {
        let obj = w.objects.entry(key.clone()).or_insert_with(|| Object {
            kind: kind.clone(),
            id: key.split_once(':').unwrap().1.into(),
            revisions: vec![],
            parent_override: false,
        });
        if matches!(command,Command::Revise {req,capability,..} if key==&address("requirement",req) && capability.as_ref().is_some_and(|cap|obj.revisions.last().is_some_and(|r|r.parent.as_deref()!=Some(&address("capability",cap)))))
        {
            obj.parent_override = false;
        }
        let parent = if obj.parent_override {
            obj.revisions.last().and_then(|v| v.parent.clone())
        } else {
            parent.clone()
        };
        let forced = forced_req.is_some_and(|req| {
            key == &address("requirement", req)
                || kind == "ac" && parent.as_deref() == Some(&address("requirement", req))
        });
        let changed = obj
            .revisions
            .last()
            .is_none_or(|v| v.data != *data || v.parent != parent || !v.present)
            || forced;
        if changed {
            let links = obj
                .revisions
                .last()
                .map(|v| v.links.clone())
                .unwrap_or_default();
            obj.revisions.push(ObjectRevision {
                revision: obj.revisions.len() as u32 + 1,
                schema_revision,
                parent,
                data: data.clone(),
                links,
                actor: actor.into(),
                at: at.into(),
                position,
                reason: reason.clone(),
                present: true,
            });
        }
    }
    for (key, obj) in &mut w.objects {
        if defaults().contains_key(&obj.kind)
            && obj.kind != "note"
            && !rows.contains_key(key)
            && obj.current().present
        {
            let mut r = obj.current().clone();
            r.revision += 1;
            r.present = false;
            r.actor = actor.into();
            r.at = at.into();
            r.position = position;
            r.reason = reason.clone();
            obj.revisions.push(r);
        }
    }
}

fn validate_links(w: &Workspace, key: &str, links: &[ObjectLink]) -> Result<()> {
    let mut unique = BTreeSet::new();
    for link in links {
        ensure!(
            ["related", "depends_on"].contains(&link.relation.as_str()),
            "unknown link relation"
        );
        ensure!(
            unique.insert((&link.target, &link.relation)),
            "duplicate object link"
        );
        let target = w
            .objects
            .get(&link.target)
            .context("unknown linked object in this workspace")?;
        ensure!(
            target.current().present,
            "linked object is absent from the current tree"
        );
        if let Some(r) = link.revision {
            ensure!(
                r > 0 && r as usize <= target.revisions.len(),
                "unknown linked revision"
            );
        }
        if link.relation == "depends_on" {
            ensure!(
                link.revision.is_some(),
                "dependency needs a target revision"
            );
        }
        if link.blocking {
            ensure!(
                link.relation == "depends_on"
                    && ["ac", "requirement", "task"].contains(&target.kind.as_str()),
                "blocking dependencies require an AC, REQ or Task target"
            );
        }
    }
    fn visit(
        w: &Workspace,
        key: &str,
        changed: &str,
        links: &[ObjectLink],
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        if done.contains(key) {
            return Ok(());
        }
        ensure!(stack.insert(key.into()), "dependency cycle");
        let current = if key == changed {
            links
        } else {
            &w.objects
                .get(key)
                .context("unknown dependency")?
                .current()
                .links
        };
        for link in current.iter().filter(|l| l.blocking) {
            visit(w, &link.target, changed, links, stack, done)?;
        }
        if let Some(parent) = w
            .objects
            .get(key)
            .and_then(|o| o.current().parent.as_deref())
        {
            visit(w, parent, changed, links, stack, done)?;
        }
        stack.remove(key);
        done.insert(key.into());
        Ok(())
    }
    visit(
        w,
        key,
        key,
        links,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )
}
fn parent_cycle(w: &Workspace, key: &str, parent: Option<&str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    seen.insert(key.to_string());
    let mut p = parent;
    while let Some(id) = p {
        ensure!(seen.insert(id.into()), "tree parent cycle");
        p = w
            .objects
            .get(id)
            .and_then(|o| o.current().parent.as_deref());
    }
    Ok(())
}
fn string(data: &Value, name: &str) -> Result<String> {
    data[name]
        .as_str()
        .map(str::to_owned)
        .with_context(|| format!("missing text field {name}"))
}
fn optional_string(data: &Value, name: &str) -> Result<Option<String>> {
    match data.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(
            v.as_str()
                .with_context(|| format!("{name} must be text"))?
                .into(),
        )),
    }
}
fn list(data: &Value, name: &str) -> Result<Vec<String>> {
    serde_json::from_value(data.get(name).cloned().unwrap_or_else(|| json!([])))
        .with_context(|| format!("{name} must contain identifiers"))
}
fn local(parent: Option<&str>, kind: &str) -> Result<String> {
    parent
        .and_then(|v| v.strip_prefix(&format!("{kind}:")))
        .map(str::to_owned)
        .with_context(|| format!("parent must be {kind}:id"))
}
fn fresh_acs(r: &Revision) -> Vec<Ac> {
    r.acs
        .iter()
        .cloned()
        .map(|mut a| {
            a.assessment = None;
            a
        })
        .collect()
}
fn revise(
    req: &str,
    r: &Revision,
    capability: Option<String>,
    data: &Value,
    reason: &str,
    acs: Vec<Ac>,
) -> Result<Command> {
    Ok(Command::Revise {
        req: req.into(),
        expected_revision: r.number,
        capability,
        consumers: list(data, "consumers")?,
        title: string(data, "title")?,
        body: string(data, "body")?,
        owner: string(data, "owner")?,
        reason: reason.into(),
        acs,
        coverage_policy: true,
        carry_tasks: true,
    })
}
fn typed_command(
    w: &Workspace,
    kind: &str,
    id: &str,
    parent: Option<&str>,
    data: &Value,
    reason: &str,
) -> Result<Option<Command>> {
    let exists = w.objects.contains_key(&address(kind, id));
    let allowed: Option<&[&str]> = match kind {
        "domain" => Some(&["title", "owner", "description", "archived"]),
        "capability" | "consumer" => Some(&["title", "owner", "description"]),
        "requirement" => Some(&["title", "body", "owner", "consumers"]),
        "ac" => Some(&[
            "title",
            "description",
            "owner",
            "decision_required",
            "depends_on",
            "before_launch",
        ]),
        "task" => Some(&["title", "owner", "contract", "projects", "purpose"]),
        _ => None,
    };
    if let Some(allowed) = allowed {
        for field in data.as_object().context("data must be an object")?.keys() {
            ensure!(
                allowed.contains(&field.as_str()),
                "unknown document field {field}; use process commands for evidence"
            );
        }
    }
    let description =
        || Ok::<_, anyhow::Error>(optional_string(data, "description")?.unwrap_or_default());
    let cmd = match kind {
        "domain" => {
            ensure!(
                data.get("archived").is_none_or(|v| Some(v)
                    == w.objects
                        .get(&address(kind, id))
                        .map(|o| &o.current().data["archived"])
                    || !exists && v == false),
                "use set_domain_archived to change archive state"
            );
            if exists {
                Command::EditDomain {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                }
            } else {
                Command::Domain {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                }
            }
        }
        "capability" => {
            if exists {
                Command::EditCapability {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: optional_string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                }
            } else {
                Command::Capability {
                    id: id.into(),
                    title: string(data, "title")?,
                    domain: ancestor(w, parent, "domain").ok(),
                    owner: optional_string(data, "owner")?,
                    description: description()?,
                }
            }
        }
        "consumer" => {
            if exists {
                Command::EditConsumer {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                    reason: reason.into(),
                }
            } else {
                Command::Consumer {
                    id: id.into(),
                    title: string(data, "title")?,
                    owner: string(data, "owner")?,
                    description: description()?,
                }
            }
        }
        "requirement" => {
            let consumers = list(data, "consumers")?;
            ensure!(!consumers.is_empty(), "REQ needs at least one Consumer");
            let capability = ancestor(w, parent, "capability").ok();
            if let Some(q) = w.requirements.get(id) {
                let r = q.revisions.last().unwrap();
                revise(id, r, capability, data, reason, fresh_acs(r))?
            } else {
                Command::Revise {
                    req: id.into(),
                    expected_revision: 0,
                    capability,
                    consumers,
                    title: string(data, "title")?,
                    body: string(data, "body")?,
                    owner: string(data, "owner")?,
                    reason: reason.into(),
                    acs: vec![],
                    coverage_policy: true,
                    carry_tasks: true,
                }
            }
        }
        "ac" => {
            let req = local(parent, "requirement")?;
            let q = w.requirements.get(&req).context("unknown parent REQ")?;
            let r = q.revisions.last().unwrap();
            if exists {
                ensure!(
                    w.objects[&address(kind, id)].current().parent == parent.map(str::to_owned),
                    "AC cannot move to another REQ"
                );
            }
            let assigned = optional_string(data, "owner")?.unwrap_or_else(|| r.owner.clone());
            commands::human(w, &assigned)?;
            let decision = data
                .get("decision_required")
                .map(|v| v.as_bool().context("decision_required must be boolean"))
                .transpose()?
                .unwrap_or(false);
            ensure!(
                decision || assigned == r.owner,
                "an independent AC owner requires decision_required=true"
            );
            let ac = Ac {
                id: id.into(),
                text: string(data, "title")?,
                description: description()?,
                owner: decision.then_some(assigned),
                depends_on: list(data, "depends_on")?,
                before_launch: data
                    .get("before_launch")
                    .map(|v| v.as_bool().context("before_launch must be boolean"))
                    .transpose()?
                    .unwrap_or(false),
                assessment: None,
            };
            let mut acs = fresh_acs(r);
            if let Some(old) = acs.iter_mut().find(|a| a.id == id) {
                *old = ac;
            } else {
                acs.push(ac);
            }
            let req_data = &w.objects[&address("requirement", &req)].current().data;
            revise(&req, r, q.capability.clone(), req_data, reason, acs)?
        }
        "task" => {
            let ac_id = local(parent, "ac")?;
            let ac = w
                .objects
                .get(&address("ac", &ac_id))
                .context("unknown AC")?;
            let req = local(ac.current().parent.as_deref(), "requirement")?;
            let r = w.requirements[&req].revisions.last().unwrap();
            ensure!(
                optional_string(data, "owner")?.is_none_or(|owner| owner == r.owner),
                "Task inherits its REQ owner"
            );
            let tid = id
                .strip_prefix(&format!("{req}/"))
                .context("task id must be <REQ>/<task>")?;
            if exists {
                let old = w.objects[&address(kind, id)].current();
                ensure!(
                    old.parent == parent.map(str::to_owned),
                    "Task cannot move to another AC"
                );
                ensure!(
                    old.data["purpose"] == data["purpose"],
                    "task purpose cannot change"
                );
                Command::AmendTask {
                    req,
                    revision: r.number,
                    id: tid.into(),
                    title: string(data, "title")?,
                    contract: string(data, "contract")?,
                    projects: list(data, "projects")?,
                    reason: reason.into(),
                }
            } else {
                Command::Task {
                    req,
                    revision: r.number,
                    id: tid.into(),
                    title: string(data, "title")?,
                    contract: string(data, "contract")?,
                    projects: list(data, "projects")?,
                    purpose: string(data, "purpose")?,
                    acs: vec![ac_id],
                    assignee: None,
                }
            }
        }
        "project" | "intake" | "point" => return Ok(None),
        _ => return Ok(None),
    };
    Ok(Some(cmd))
}
fn ancestor(w: &Workspace, parent: Option<&str>, kind: &str) -> Result<String> {
    let mut p = parent;
    let mut seen = BTreeSet::new();
    while let Some(key) = p {
        ensure!(seen.insert(key), "tree parent cycle");
        let o = w.objects.get(key).context("unknown ancestor")?;
        if o.kind == kind {
            return Ok(o.id.clone());
        }
        p = o.current().parent.as_deref();
    }
    bail!("semantic {kind} ancestor is required")
}
pub fn save(
    w: &mut Workspace,
    actor: &str,
    command: &Command,
    position: u64,
    at: &str,
) -> Result<()> {
    let Command::SaveObject {
        id,
        kind,
        revision,
        parent,
        data,
        links,
        reason,
    } = command
    else {
        bail!("save_object required")
    };
    commands::nonempty(reason)?;
    ensure!(
        !id.is_empty()
            && id.len() <= 200
            && !id.contains(':')
            && !id.chars().any(char::is_whitespace),
        "invalid object id"
    );
    commands::identifier(kind)?;
    let key = address(kind, id);
    let old = w.objects.get(&key).cloned();
    ensure!(
        old.as_ref().map_or(0, |o| o.current().revision) == *revision,
        "stale object revision; read the object again"
    );
    if let Some(old) = &old {
        ensure!(
            old.current().present,
            "object is absent from the current tree"
        );
    }
    ensure!(data.is_object(), "data must be an object");
    commands::nonempty(&string(data, "title")?)?;
    parent_valid(&schema(w).types, w, kind, parent.as_deref())?;
    parent_cycle(w, &key, parent.as_deref())?;
    validate_links(w, &key, links)?;
    let unchanged = old
        .as_ref()
        .is_some_and(|o| o.current().data == *data && o.current().parent == *parent);
    let mut candidate = w.clone();
    if !unchanged || kind == "requirement" {
        if let Some(cmd) = typed_command(w, kind, id, parent.as_deref(), data, reason)? {
            commands::apply_inner(&mut candidate, actor, &cmd, position, at)?;
            sync(&mut candidate, actor, &cmd, position, at);
        } else {
            let owner = string(data, "owner")?;
            commands::human(w, &owner)?;
            if let Some(o) = &old {
                ensure!(
                    o.current().data["owner"] == owner
                        || o.current().data["owner"] == actor
                        || actor == w.admin,
                    "only current owner can transfer responsibility"
                );
            }
            if ["project", "intake", "point"].contains(&kind.as_str()) {
                ensure!(
                    old.is_some(),
                    "create source data with save_object kind=note"
                );
                ensure!(
                    actor == w.admin
                        || old
                            .as_ref()
                            .is_some_and(|o| o.current().data["owner"] == actor),
                    "source owner required"
                );
                match kind.as_str() {
                    "project" => {
                        ensure!(
                            string(data, "title")? == *id,
                            "project title is its stable identifier"
                        );
                        candidate
                            .projects
                            .insert(id.clone(), string(data, "repository")?);
                    }
                    "intake" => {
                        let i = candidate.intake.get_mut(id).context("unknown intake")?;
                        ensure!(
                            data["author"] == i.author && data["title"] == *id,
                            "source author and id cannot change"
                        );
                        i.said = string(data, "body")?;
                        i.owner = Some(owner.clone());
                    }
                    "point" => {
                        let (source, point) = id.split_once('/').context("invalid source point")?;
                        let p = candidate
                            .intake
                            .get_mut(source)
                            .and_then(|i| i.points.get_mut(point))
                            .context("unknown source point")?;
                        ensure!(
                            old.as_ref().unwrap().current().data["owner"] == owner,
                            "point inherits its source owner"
                        );
                        for field in ["kind", "answers", "outcome", "reason", "requirement"] {
                            ensure!(
                                data[field] == old.as_ref().unwrap().current().data[field],
                                "use source process commands for {field}"
                            );
                        }
                        p.said = string(data, "title")?;
                    }
                    _ => unreachable!(),
                }
                sync(&mut candidate, actor, command, position, at);
            }
        }
    }
    let schema_revision = schema(w).revision;
    let projected = candidate
        .objects
        .get(&key)
        .map(|o| o.current().data.clone())
        .unwrap_or_else(|| data.clone());
    // A custom parent changes grouping. The legacy semantic ancestor remains explicit.
    let mut object = candidate
        .objects
        .get(&key)
        .cloned()
        .unwrap_or_else(|| Object {
            id: id.clone(),
            kind: kind.clone(),
            revisions: vec![],
            parent_override: false,
        });
    object.revisions = old.map(|o| o.revisions).unwrap_or_default();
    let saved_data = if defaults().contains_key(kind) && kind != "note" {
        projected
    } else {
        data.clone()
    };
    object.parent_override = document_rows(&candidate)
        .get(&key)
        .is_none_or(|(_, p, _)| p != parent);
    object.revisions.push(ObjectRevision {
        revision: revision + 1,
        schema_revision,
        parent: parent.clone(),
        data: saved_data,
        links: links.clone(),
        actor: actor.into(),
        at: at.into(),
        position,
        reason: reason.clone(),
        present: true,
    });
    candidate.objects.insert(key, object);
    *w = candidate;
    Ok(())
}

pub fn review_links(w: &Workspace, key: &str) -> Vec<Value> {
    w.objects.get(key).map(|o|o.current().links.iter().filter_map(|link| {
        let current=w.objects.get(&link.target).filter(|o|o.current().present).map(|o|o.current().revision);
        (link.relation=="depends_on" && current!=link.revision).then(||json!({"target":link.target,"revision":link.revision,"current_revision":current}))
    }).collect()).unwrap_or_default()
}
pub fn work_ready(w: &Workspace, key: &str) -> bool {
    fn visit(w: &Workspace, key: &str, stack: &mut BTreeSet<String>) -> bool {
        let Some(o) = w.objects.get(key) else {
            return false;
        };
        if !o.current().present
            || o.current().links.iter().any(|l| {
                l.blocking
                    && w.objects.get(&l.target).is_none_or(|target| {
                        !target.current().present || Some(target.current().revision) != l.revision
                    })
            })
            || !stack.insert(key.into())
        {
            return false;
        }
        let links_ready = o.current().links.iter().filter(|l| l.blocking).all(|l| {
            let Some(target) = w.objects.get(&l.target) else {
                return false;
            };
            let evidence = match target.kind.as_str() {
                "ac" => target
                    .current()
                    .parent
                    .as_deref()
                    .and_then(|p| p.strip_prefix("requirement:"))
                    .and_then(|req| w.requirements.get(req))
                    .and_then(|q| q.revisions.last())
                    .is_some_and(|r| {
                        crate::checks::latest(r).is_some_and(|(_, run)| {
                            crate::checks::verdict(r, run, &target.id) == "pass"
                        })
                    }),
                "requirement" => w
                    .requirements
                    .get(&target.id)
                    .and_then(|q| q.revisions.last())
                    .is_some_and(|r| crate::checks::summary(r)["accepted"] == true),
                "task" => target
                    .id
                    .split_once('/')
                    .and_then(|(req, id)| {
                        w.requirements
                            .get(req)
                            .and_then(|q| q.revisions.last())
                            .and_then(|r| r.tasks.get(id))
                    })
                    .is_some_and(|t| t.done.is_some() && t.retired.is_none()),
                _ => false,
            };
            evidence && visit(w, &l.target, stack)
        });
        let parent_ready = o
            .current()
            .parent
            .as_deref()
            .is_none_or(|p| stack.contains(p) || visit(w, p, stack));
        stack.remove(key);
        links_ready && parent_ready
    }
    visit(w, key, &mut BTreeSet::new())
}

fn legacy_task_binding(w: &Workspace, obj: &Object) -> bool {
    obj.kind == "task"
        && obj.id.split_once('/').is_some_and(|(req, id)| {
            obj.current().parent.as_deref() == Some(&address("requirement", req))
                && w.requirements
                    .get(req)
                    .and_then(|q| q.revisions.last())
                    .and_then(|r| r.tasks.get(id))
                    .is_some_and(|t| {
                        t.acs.len() != 1 && obj.current().data["legacy_acs"] == json!(t.acs)
                    })
        })
}

/// New writes validate the active schema. Replay preserves earlier schema rules.
pub fn validate_tree(w: &Workspace) -> Result<()> {
    let types = schema(w).types;
    for (key, o) in w.objects.iter().filter(|(_, o)| o.current().present) {
        ensure!(
            types.contains_key(&o.kind),
            "object type is absent from the tree schema"
        );
        if !legacy_task_binding(w, o) {
            parent_valid(&types, w, &o.kind, o.current().parent.as_deref())?;
        }
        parent_cycle(w, key, o.current().parent.as_deref())?;
    }
    Ok(())
}

pub fn requirement_summary(w: &Workspace, req: &str, r: &Revision) -> Value {
    let mut summary = crate::checks::summary(r);
    let mut blocked = false;
    for ac in &r.acs {
        let key = format!("ac:{}", ac.id);
        if !w.objects.is_empty() && !work_ready(w, &key) {
            summary["acs"][&ac.id]["ready"] = json!(false);
            summary["acs"][&ac.id]["verification"] = json!("n/a");
            summary["acs"][&ac.id]["review_required"] = json!(review_links(w, &key));
            blocked = true;
        }
    }
    if blocked || !w.objects.is_empty() && !work_ready(w, &format!("requirement:{req}")) {
        summary["ready_for_acceptance"] = json!(false);
        summary["accepted"] = json!(false);
    }
    summary
}

pub fn within(w: &Workspace, key: &str, target: &str) -> bool {
    let mut current = Some(key);
    let mut seen = BTreeSet::new();
    while let Some(id) = current {
        if id == target {
            return true;
        }
        if !seen.insert(id) {
            return false;
        }
        current = w
            .objects
            .get(id)
            .and_then(|o| o.current().parent.as_deref());
    }
    false
}

pub fn validate_current(w: &Workspace, cmd: &Command) -> Result<()> {
    match cmd {
        Command::Task { acs, .. } => ensure!(acs.len() == 1, "Task must belong to exactly one AC"),
        Command::CompleteTask { req, id, .. } => ensure!(
            work_ready(w, &format!("task:{req}/{id}")),
            "Task has an unmet dependency or a changed dependency revision"
        ),
        Command::ReviewTask {
            req,
            revision,
            id,
            ac,
            ..
        } => {
            let r = w
                .requirements
                .get(req)
                .and_then(|q| q.revisions.last())
                .context("unknown REQ")?;
            ensure!(r.number == *revision, "stale REQ revision");
            let task = r.tasks.get(id).context("unknown Task")?;
            ensure!(
                task.done.is_none() || task.acs == vec![ac.clone()],
                "completed Task cannot move to another AC"
            );
            ensure!(task.retired.is_none(), "retired Task cannot be reviewed");
        }
        Command::Accept { req, .. } => {
            let r = w
                .requirements
                .get(req)
                .and_then(|q| q.revisions.last())
                .context("unknown REQ")?;
            ensure!(
                requirement_summary(w, req, r)["ready_for_acceptance"] == true,
                "REQ has unverified ACs or an unmet dependency"
            );
        }
        _ => {}
    }
    Ok(())
}
pub fn annotation_address(w: &Workspace, key: &str) -> String {
    if let Some(id) = key.strip_prefix("ac:")
        && !id.contains('/')
        && let Some(req) = w
            .objects
            .get(key)
            .and_then(|o| o.current().parent.as_deref())
            .and_then(|p| p.strip_prefix("requirement:"))
    {
        return format!("ac:{req}/{id}");
    }
    key.into()
}

pub fn process(w: &Workspace, key: &str) -> Value {
    let Some(o) = w.objects.get(key) else {
        return json!({});
    };
    match o.kind.as_str() {
        "requirement"=>w.requirements.get(&o.id).and_then(|q|q.revisions.last()).map(|r|json!({"req":o.id,"revision":r.number,"confirmed":r.confirmed,"summary":requirement_summary(w,&o.id,r)})).unwrap_or_else(||json!({})),
        "ac"=>o.current().parent.as_deref().and_then(|p|p.strip_prefix("requirement:")).and_then(|req|w.requirements.get(req).and_then(|q|q.revisions.last()).map(|r|(req,r))).map(|(req,r)|json!({"req":req,"revision":r.number,"confirmed":r.confirmed,"pending_run":requirement_summary(w,req,r)["pending_run"],"status":requirement_summary(w,req,r)["acs"][&o.id]})).unwrap_or_else(||json!({})),
        "task"=>o.id.split_once('/').and_then(|(req,id)|w.requirements.get(req).and_then(|q|q.revisions.last()).and_then(|r|r.tasks.get(id).map(|t|(req,r,t)))).map(|(req,r,t)|json!({"req":req,"revision":r.number,"assignee":t.assignee,"done":t.done,"retired":t.retired,"review_required":t.review_required,"status":if t.retired.is_some(){"retired"}else if t.done.is_some(){"done"}else if t.assignee.is_some(){"in_progress"}else{"open"}})).unwrap_or_else(||json!({})),
        _=>json!({}),
    }
}
pub fn signal(w: &Workspace, key: &str) -> Value {
    let Some(o) = w.objects.get(key) else {
        return json!({});
    };
    let p = process(w, key);
    let (tone, label) = if !o.current().present {
        (
            "grey",
            "Object is absent from the current verification revision",
        )
    } else if !review_links(w, key).is_empty() {
        ("purple", "Dependency revision changed")
    } else if !work_ready(w, key) {
        ("purple", "Dependency is not ready")
    } else if o.kind == "ac" && p["pending_run"].is_string() {
        ("yellow", "New checks are running")
    } else if o.kind == "ac" {
        match (
            p["status"]["ready"].as_bool(),
            p["status"]["verification"].as_str(),
        ) {
            (_, Some("fail")) => ("red", "Checks failed"),
            (Some(true), Some("pass")) => ("green", "Checks passed"),
            (Some(false), _) => ("yellow", "AC is not ready"),
            _ => ("grey", "AC awaits checks"),
        }
    } else if o.kind == "requirement" {
        if p["summary"]["acs"]
            .as_object()
            .is_some_and(|acs| acs.values().any(|a| a["verification"] == "fail"))
        {
            ("red", "AC checks failed")
        } else if p["summary"]["pending_run"].is_string() {
            ("yellow", "New checks are running")
        } else if p["summary"]["accepted"] == true {
            ("green", "REQ accepted")
        } else if p["summary"]["ready_for_acceptance"] == true {
            ("green", "AC checks passed")
        } else if p["confirmed"] == false {
            ("yellow", "REQ awaits confirmation")
        } else {
            ("grey", "REQ awaits checks")
        }
    } else if o.kind == "task" {
        match p["status"].as_str() {
            Some("done") => ("grey", "Task completed"),
            Some("retired") => ("grey", "Task retired"),
            Some("in_progress") => ("yellow", "Task in progress"),
            _ => ("grey", "Task open"),
        }
    } else {
        ("grey", "Current document")
    };
    json!({"tone":tone,"label":label,"at":o.current().at})
}
