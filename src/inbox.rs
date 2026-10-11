//! A decision on an Inbox signal: changed_spec, needs_information, rejected or duplicate.
//! The decision is saved into the note itself, so the note keeps its own history.
//! needs_information asks the author and keeps the signal open; the other outcomes close it.
//! Only a person may reject a signal or mark it duplicate; an agent may only propose those.
use crate::{commands::Command, model::Workspace};
use anyhow::{Context, Result, bail, ensure};
use serde_json::json;

pub struct Decision<'a> {
    pub target: &'a str,
    pub revision: u32,
    pub outcome: &'a str,
    pub reason: &'a str,
    pub links: &'a [String],
    pub duplicate_of: Option<&'a str>,
    pub question: Option<&'a str>,
    pub recipient: Option<&'a str>,
}

/// A member with role human is a person; agents and runners are not.
pub fn actor_type(w: &Workspace, actor: &str) -> &'static str {
    match w.members.get(actor).map(String::as_str) {
        Some("human") => "person",
        _ if actor == w.admin => "person",
        _ => "agent",
    }
}

pub fn decide(w: &mut Workspace, actor: &str, d: &Decision, position: u64, at: &str) -> Result<()> {
    crate::commands::nonempty(d.reason)?;
    ensure!(
        d.recipient.is_none() || d.outcome == "needs_information",
        "recipient requires needs_information"
    );
    let key = crate::objects::resolve_address(w, d.target).to_owned();
    let note = w.objects.get(&key).context("unknown Note")?;
    ensure!(
        crate::objects::input_type(w, &note.kind),
        "decide_note requires an inbox type"
    );
    let kind = actor_type(w, actor);
    let mut decision =
        json!({"outcome": d.outcome, "reason": d.reason, "actor": actor, "actor_type": kind});
    let mut links = note.current().links.clone();
    let mut link = |target: &str| -> Result<()> {
        ensure!(
            w.objects.contains_key(target),
            "unknown linked object in this workspace"
        );
        if !links.iter().any(|l| l.target == target) {
            links.push(crate::objects::ObjectLink {
                target: target.into(),
                relation: "related".into(),
                revision: None,
                blocking: false,
            });
        }
        Ok(())
    };
    match d.outcome {
        "changed_spec" => {
            ensure!(
                !d.links.is_empty(),
                "changed_spec needs links to the changed objects"
            );
            for target in d.links {
                link(target)?;
            }
        }
        "needs_information" => {
            let question = d
                .question
                .filter(|q| !q.trim().is_empty())
                .context("needs_information needs a question")?;
            let data = &note.current().data;
            let asked = if let Some(recipient) = d.recipient {
                crate::commands::human(w, recipient)?;
                recipient
            } else {
                data["source"]["author"]
                    .as_str()
                    .or(data["owner"].as_str())
                    .unwrap_or_default()
            };
            decision["question"] = json!(question);
            decision["asked"] = json!(asked);
        }
        "rejected" | "duplicate" => {
            ensure!(
                kind == "person",
                "only a person decides on a signal from a person"
            );
            if d.outcome == "duplicate" {
                let original = d.duplicate_of.context("duplicate needs duplicate_of")?;
                link(original)?;
                decision["duplicate_of"] = json!(original);
            }
        }
        _ => bail!("unknown decision outcome"),
    }
    let r = note.current();
    let mut data = r.data.clone();
    data["decision"] = decision;
    data["status"] = json!(if d.outcome == "needs_information" {
        "unprocessed"
    } else {
        "processed"
    });
    let command = Command::SaveObject {
        question_checked: None,
        kind: note.kind.clone(),
        id: note.id.clone(),
        revision: d.revision,
        parent: r.parent.clone(),
        data,
        links,
        reason: d.reason.into(),
    };
    crate::objects::save(w, actor, &command, position, at)
}
