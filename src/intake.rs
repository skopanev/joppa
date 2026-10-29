use crate::{
    commands::{Command, identifier, nonempty},
    model::*,
};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub fn apply(w: &mut Workspace, actor: &str, cmd: &Command) -> Result<()> {
    match cmd {
        Command::Intake(crate::commands::IntakeOp {
            id,
            said,
            author,
            owner,
        }) => {
            identifier(id)?;
            nonempty(author)?;
            if let Some(owner) = owner {
                crate::commands::human(w, owner)?;
            }
            let assigned = owner.clone().unwrap_or_else(|| {
                if w.members.get(actor).is_some_and(|role| role == "human") {
                    actor.to_owned()
                } else {
                    w.admin.clone()
                }
            });
            ensure!(!w.intake.contains_key(id), "intake exists");
            // Deliberately no trim, minimum length or format constraint on the source text.
            w.intake.insert(
                id.clone(),
                Intake {
                    said: said.clone(),
                    author: author.clone(),
                    owner: Some(assigned),
                    points: BTreeMap::new(),
                },
            );
        }
        Command::Split(crate::commands::SplitOp { id, points }) => {
            let intake = w
                .intake
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("unknown intake"))?;
            ensure!(intake.points.is_empty(), "intake already split");
            ensure!(!points.is_empty(), "supply at least one point");
            for (i, said) in points.iter().enumerate() {
                intake.points.insert(
                    format!("{}.{}", id, i + 1),
                    Point {
                        said: said.clone(),
                        kind: String::new(),
                        answers: BTreeMap::new(),
                        outcome: None,
                        reason: None,
                        requirement: None,
                    },
                );
            }
        }
        Command::Questions(crate::commands::QuestionsOp { kind, fields }) => {
            ensure!(actor == w.admin, "workspace owner changes intake policy");
            ensure!(
                ["bug", "policy", "question", "idea"].contains(&kind.as_str()),
                "unknown intake kind"
            );
            ensure!(!fields.is_empty(), "empty policy");
            for f in fields {
                identifier(f)?;
            }
            for intake in w.intake.values_mut() {
                for point in intake.points.values_mut() {
                    if point.kind == *kind && point.outcome.is_none() {
                        point.answers.clear();
                    }
                }
            }
            w.questions.insert(kind.clone(), fields.clone());
        }
        Command::Sort(crate::commands::SortOp { id, point, kind }) => {
            ensure!(w.questions.contains_key(kind), "unknown intake kind");
            let p = point_mut(w, id, point)?;
            ensure!(p.outcome.is_none(), "already routed");
            if p.kind != *kind {
                p.answers.clear();
            } // Old answers remain in the event history.
            p.kind = kind.clone();
        }
        Command::Answer(crate::commands::AnswerOp {
            id,
            point,
            field,
            value,
        }) => {
            nonempty(value)?;
            let p = point_mut(w, id, point)?;
            let kind = p.kind.clone();
            ensure!(
                w.questions.get(&kind).is_some_and(|fs| fs.contains(field)),
                "field not in intake policy"
            );
            let p = point_mut(w, id, point)?;
            ensure!(p.outcome.is_none(), "already routed");
            p.answers.insert(field.clone(), value.clone());
        }
        Command::Route(crate::commands::RouteOp {
            id,
            point,
            outcome,
            reason,
            req,
        }) => {
            nonempty(reason)?;
            ensure!(
                ["work", "refused", "elsewhere"].contains(&outcome.as_str()),
                "unknown route"
            );
            if outcome == "work" {
                ensure!(
                    req.as_ref().is_some_and(|r| w.requirements.contains_key(r)),
                    "work needs an existing REQ; no automatic REQ for bugs"
                );
                let p = point_mut(w, id, point)?;
                let kind = p.kind.clone();
                let answers = p.answers.clone();
                let fields = w
                    .questions
                    .get(&kind)
                    .ok_or_else(|| anyhow::anyhow!("classify the point first"))?;
                ensure!(
                    fields
                        .iter()
                        .all(|f| answers.get(f).is_some_and(|v| !v.trim().is_empty())),
                    "required intake answers missing"
                );
            } else {
                ensure!(req.is_none(), "refusal/elsewhere cannot bind work");
            }
            let p = point_mut(w, id, point)?;
            ensure!(
                p.outcome.is_none(),
                "point already routed; preserve decision and file follow-up intake"
            );
            p.outcome = Some(outcome.clone());
            p.reason = Some(reason.clone());
            p.requirement = req.clone();
        }
        _ => return crate::commands::unknown(),
    }
    Ok(())
}
fn point_mut<'a>(w: &'a mut Workspace, id: &str, point: &str) -> Result<&'a mut Point> {
    w.intake
        .get_mut(id)
        .and_then(|i| i.points.get_mut(point))
        .ok_or_else(|| anyhow::anyhow!("unknown intake point"))
}
