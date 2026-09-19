use crate::{checks, model::*};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Workspace {
        title: String,
    },
    Member {
        id: String,
        role: String,
    },
    Domain {
        id: String,
        title: String,
        authority: String,
    },
    Capability {
        id: String,
        title: String,
        domain: String,
    },
    ConfirmTree {},
    EditDomain {
        id: String,
        title: String,
        authority: String,
        reason: String,
    },
    EditCapability {
        id: String,
        title: String,
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
        capability: String,
        title: String,
        body: String,
        owner: String,
        reason: String,
        acs: Vec<Ac>,
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
        assignee: String,
    },
    CompleteTask {
        req: String,
        revision: u32,
        id: String,
        evidence: String,
    },
    Plan {
        req: String,
        revision: u32,
        run: String,
        subject: BTreeMap<String, String>,
        scope: String,
        discovery_complete: bool,
        checks: Vec<Check>,
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
pub fn apply(w: &mut Workspace, actor: &str, command: &Command, position: u64) -> Result<()> {
    identifier(actor)?;
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
    match command {
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
        Command::Domain {
            id,
            title,
            authority,
        } => {
            identifier(id)?;
            nonempty(title)?;
            human(w, authority)?;
            ensure!(!w.domains.contains_key(id), "domain exists");
            w.domains.insert(
                id.clone(),
                Domain {
                    title: title.clone(),
                    authority: authority.clone(),
                    confirmed: actor == w.admin,
                },
            );
        }
        Command::Capability { id, title, domain } => {
            identifier(id)?;
            nonempty(title)?;
            ensure!(w.domains.contains_key(domain), "unknown domain");
            ensure!(!w.capabilities.contains_key(id), "capability exists");
            w.capabilities.insert(
                id.clone(),
                Capability {
                    title: title.clone(),
                    domain: domain.clone(),
                    confirmed: actor == w.admin,
                },
            );
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
            authority,
            reason,
        } => {
            ensure!(actor == w.admin, "workspace owner edits domain authority");
            nonempty(title)?;
            nonempty(reason)?;
            human(w, authority)?;
            let d = w
                .domains
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown domain"))?;
            d.title = title.clone();
            d.authority = authority.clone();
        }
        Command::EditCapability { id, title, reason } => {
            ensure!(actor == w.admin, "workspace owner edits capability");
            nonempty(title)?;
            nonempty(reason)?;
            w.capabilities
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown capability"))?
                .title = title.clone();
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
                            recipient: Some(actor.into()),
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
            title,
            body,
            owner,
            reason,
            acs,
        } => {
            identifier(req)?;
            nonempty(title)?;
            nonempty(reason)?;
            human(w, owner)?;
            ensure!(
                w.capabilities.contains_key(capability),
                "unknown capability"
            );
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
            ensure!(
                r.capability == *capability,
                "REQ cannot move silently to another capability"
            );
            if let Some(old) = r.revisions.last() {
                ensure!(
                    old.owner == *owner || actor == old.owner,
                    "only current owner can transfer responsibility"
                );
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
                title: title.clone(),
                body: body.clone(),
                owner: owner.clone(),
                reason: reason.clone(),
                acs,
                confirmed: false,
                tasks: BTreeMap::new(),
                runs: BTreeMap::new(),
                issues: BTreeMap::new(),
                acceptance: None,
            });
        }
        Command::Confirm { req, revision: n } => {
            let q = w
                .requirements
                .get(req)
                .ok_or_else(|| anyhow::anyhow!("unknown REQ"))?;
            let c = &w.capabilities[&q.capability];
            ensure!(
                c.confirmed && w.domains[&c.domain].confirmed,
                "workspace owner must confirm the business tree first"
            );
            let r = revision(w, req, *n, true)?;
            ensure!(r.owner == actor, "REQ owner confirms its formulation");
            r.confirmed = true;
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
                    .recipient
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("waiting needs a recipient"))?;
                ensure!(w.members.contains_key(recipient), "unknown recipient");
            }
            if assessment.ready && a.decider.is_some() {
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
                                    && old.decider == a.decider
                                    && old.depends_on == a.depends_on
                                    && old.before_launch == a.before_launch
                            })
                    });
                ensure!(
                    decision.kind == "decision"
                        && !decision.withdrawn
                        && Some(&decision.actor) == a.decider.as_ref()
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
        _ => checks::apply(w, actor, command, position)?,
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
        if let Some(d) = &ac.decider {
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
