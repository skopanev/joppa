// Part 5 of tests/objects.rs, included there, so the test names stay unchanged.

// check-list-frozen-ac2
#[test]
fn a_native_revise_of_one_criterion_keeps_sibling_readiness_live_and_on_replay() {
    let mut changed = revision(1);
    changed["acs"][0]["text"] = json!("On iOS paid access survives every login");
    let steps: Vec<(&str, Value)> = setup()
        .into_iter()
        .map(|command| ("owner", command))
        .chain([
            ("owner", json!({"op":"confirm","req":"r","revision":1})),
            ("agent", assess("ios", true)),
            ("agent", assess("key", true)),
            ("owner", changed),
        ])
        .collect();
    let mut live = World {
        state: Default::default(),
        position: 0,
    };
    for (actor, command) in &steps {
        live.ok(actor, command.clone());
    }
    let mut replayed = joppa::model::Workspace::default();
    for (position, (actor, command)) in steps.iter().enumerate() {
        joppa::commands::apply_replay(
            &mut replayed,
            actor,
            &serde_json::from_value(command.clone()).unwrap(),
            position as u64 + 1,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
    }
    for w in [&live.state, &replayed] {
        let state = |ac: &str| objects::assessment(w, ac).unwrap()["state"].clone();
        assert_eq!(state("ac:ios"), "unassessed");
        assert_eq!(state("ac:key"), "ready", "a sibling keeps its readiness");
        assert_eq!(
            w.criterion_boundaries.keys().collect::<Vec<_>>(),
            vec!["ac:ios"]
        );
    }
}

/// Replay `steps` at positions 1..N with the given replay epochs.
fn replay(steps: &[(&str, Value)], epochs: &[(&str, u64)]) -> joppa::model::Workspace {
    let mut w = joppa::model::Workspace {
        replay_epochs: epochs
            .iter()
            .map(|(rule, first)| (rule.to_string(), *first))
            .collect(),
        ..Default::default()
    };
    for (position, (actor, command)) in steps.iter().enumerate() {
        joppa::commands::apply_replay(
            &mut w,
            actor,
            &serde_json::from_value(command.clone()).unwrap(),
            position as u64 + 1,
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
    }
    w
}

fn readiness(w: &joppa::model::Workspace, ac: &str) -> Value {
    objects::assessment(w, ac).unwrap()["state"].clone()
}

// A replay gives each event the meaning of the rules at its position. A revise before the
// keep_assessments and reset_changed epochs clears every assessment of its requirement.
// The same revise at the epochs keeps the readiness of the criterion that did not change.
#[test]
fn a_revise_before_its_replay_epochs_clears_every_assessment_of_the_requirement() {
    use joppa::commands::{KEEP_ASSESSMENTS, LIST_SAVE_RESET, RESET_CHANGED};
    let mut changed = revision(1);
    changed["acs"][0]["text"] = json!("On iOS paid access survives every login");
    let steps: Vec<(&str, Value)> = setup()
        .into_iter()
        .map(|command| ("owner", command))
        .chain([
            ("owner", json!({"op":"confirm","req":"r","revision":1})),
            ("agent", assess("ios", true)),
            ("agent", assess("key", true)),
            ("owner", changed),
        ])
        .collect();
    let revise = steps.len() as u64;
    let rules = |first: u64| {
        [
            (RESET_CHANGED, first),
            (KEEP_ASSESSMENTS, first),
            (LIST_SAVE_RESET, first),
        ]
    };
    let old = replay(&steps, &rules(revise + 1));
    assert_eq!(readiness(&old, "ac:ios"), "unassessed");
    assert_eq!(
        readiness(&old, "ac:key"),
        "unassessed",
        "the old revise cleared every assessment"
    );
    let new = replay(&steps, &rules(revise));
    assert_eq!(readiness(&new, "ac:ios"), "unassessed");
    assert_eq!(
        readiness(&new, "ac:key"),
        "ready",
        "a sibling keeps its readiness"
    );
}

// A test-list save before the list_save_reset epoch keeps readiness, as it did when it was
// written. At the epoch, and in every live write, the save resets the criterion.
#[test]
fn a_list_save_before_its_replay_epoch_keeps_readiness() {
    use joppa::commands::LIST_SAVE_RESET;
    let save = json!({"op":"set_ac_tests","target":"ac:ios","tests":["tests/access.rs > opens"],"reason":"Bind the test"});
    let steps: Vec<(&str, Value)> = setup()
        .into_iter()
        .map(|command| ("owner", command))
        .chain([
            ("owner", json!({"op":"confirm","req":"r","revision":1})),
            ("agent", assess("ios", true)),
            ("owner", save.clone()),
        ])
        .collect();
    let position = steps.len() as u64;
    assert_eq!(
        readiness(
            &replay(&steps, &[(LIST_SAVE_RESET, position + 1)]),
            "ac:ios"
        ),
        "ready"
    );
    assert_eq!(
        readiness(&replay(&steps, &[(LIST_SAVE_RESET, position)]), "ac:ios"),
        "unassessed"
    );
    // A live write ignores the epochs: the rule is always in force for new events.
    let mut live = World {
        state: Default::default(),
        position: 0,
    };
    for (actor, command) in &steps[..steps.len() - 1] {
        live.ok(actor, command.clone());
    }
    live.state.replay_epochs = [(LIST_SAVE_RESET.to_string(), position + 1)].into();
    live.ok("owner", save);
    assert_eq!(readiness(&live.state, "ac:ios"), "unassessed");
}
