// Part 1 of tests/inbox_rules.rs, included there, so the test names stay unchanged.

fn world() -> World {
    let mut w = World::new();
    w.ok("owner", json!({"op":"member","id":"alex","role":"human"}));
    signal(&mut w, "a", "maria");
    signal(&mut w, "b", "maria");
    w
}

fn signal(w: &mut World, id: &str, author: &str) {
    w.ok("agent", json!({"op":"save_object","kind":"note","id":id,"revision":0,"parent":null,"data":{"title":format!("Signal {id}"),"owner":"owner","body":"Refunds are slow","source":{"author":author}},"links":[],"reason":"Intake"}));
}

fn decide(w: &World, id: &str, outcome: &str, extra: Value) -> Value {
    let revision = w.state.objects[&format!("note:{id}")].current().revision;
    let mut command = json!({"op":"decide_note","target":format!("note:{id}"),"revision":revision,"outcome":outcome,"reason":"Reviewed"});
    for (k, v) in extra.as_object().unwrap() {
        command[k] = v.clone();
    }
    command
}

fn data(w: &World, id: &str) -> Value {
    w.state.objects[&format!("note:{id}")]
        .current()
        .data
        .clone()
}

fn refused(w: &mut World, actor: &str, command: Value, error: &str) {
    let e = w
        .apply(actor, command.clone())
        .expect_err(&format!("{command} must be refused"));
    assert!(e.to_string().contains(error), "{command}: {e}");
}

#[test]
fn a_decision_stores_its_outcome_and_reason() {
    // signal-decision-outcomes-ac1
    let mut w = world();
    let mut command = decide(&w, "a", "changed_spec", json!({"links":["ac:ios"]}));
    command["reason"] = json!("Added AC for refunds");
    w.ok("agent", command);
    let d = &data(&w, "a")["decision"];
    assert_eq!(
        (d["outcome"].as_str(), d["reason"].as_str()),
        (Some("changed_spec"), Some("Added AC for refunds"))
    );
    assert_eq!(data(&w, "a")["status"], "processed");
}

#[test]
fn an_unknown_outcome_is_refused() {
    // signal-decision-outcomes-ac2
    let mut w = world();
    let command = decide(&w, "a", "maybe", json!({}));
    refused(&mut w, "agent", command, "unknown decision outcome");
}

#[test]
fn needs_information_asks_the_author_and_keeps_the_signal_open() {
    // needs-information-ac1
    let mut w = world();
    w.ok(
        "agent",
        decide(
            &w,
            "a",
            "needs_information",
            json!({"question":"Which plan?"}),
        ),
    );
    let d = data(&w, "a");
    assert_eq!(d["decision"]["outcome"], "needs_information");
    assert_eq!(d["decision"]["asked"], "maria");
    assert_eq!(d["decision"]["question"], "Which plan?");
    assert_eq!(d["status"], "unprocessed");
}

#[test]
fn needs_information_without_a_question_is_refused() {
    // needs-information-ac2
    let mut w = world();
    let command = decide(&w, "a", "needs_information", json!({}));
    refused(
        &mut w,
        "agent",
        command,
        "needs_information needs a question",
    );
}

#[test]
fn changed_spec_links_each_changed_object() {
    // decided-links-changes-ac1
    let mut w = world();
    w.ok(
        "agent",
        decide(
            &w,
            "a",
            "changed_spec",
            json!({"links":["requirement:r","ac:ios"]}),
        ),
    );
    let links: Vec<String> = w.state.objects["note:a"]
        .current()
        .links
        .iter()
        .map(|l| l.target.clone())
        .collect();
    assert!(
        links.contains(&"requirement:r".into()) && links.contains(&"ac:ios".into()),
        "{links:?}"
    );
}

#[test]
fn changed_spec_without_links_is_refused() {
    // decided-links-changes-ac2
    let mut w = world();
    let command = decide(&w, "a", "changed_spec", json!({}));
    refused(
        &mut w,
        "agent",
        command,
        "changed_spec needs links to the changed objects",
    );
}

#[test]
fn a_duplicate_links_to_the_original_signal() {
    // duplicate-links-original-ac1
    let mut w = world();
    w.ok(
        "alex",
        decide(&w, "b", "duplicate", json!({"duplicate_of":"note:a"})),
    );
    let d = data(&w, "b");
    assert_eq!(
        (
            d["decision"]["outcome"].as_str(),
            d["decision"]["duplicate_of"].as_str()
        ),
        (Some("duplicate"), Some("note:a"))
    );
    assert!(
        w.state.objects["note:b"]
            .current()
            .links
            .iter()
            .any(|l| l.target == "note:a")
    );
}

#[test]
fn a_duplicate_without_an_original_is_refused() {
    // duplicate-links-original-ac2
    let mut w = world();
    let command = decide(&w, "b", "duplicate", json!({}));
    refused(&mut w, "alex", command, "duplicate needs duplicate_of");
}

#[test]
fn a_signal_stays_open_until_a_final_decision() {
    // open-until-decided-ac1
    let mut w = world();
    assert_eq!(
        data(&w, "a")["status"],
        "unprocessed",
        "a new signal is open"
    );
    w.ok(
        "agent",
        decide(
            &w,
            "a",
            "needs_information",
            json!({"question":"Which plan?"}),
        ),
    );
    assert_eq!(data(&w, "a")["status"], "unprocessed");
    w.ok("alex", decide(&w, "a", "rejected", json!({})));
    assert_eq!(data(&w, "a")["status"], "processed");
}

#[test]
fn every_decision_records_who_made_it() {
    // decision-records-actor-ac1, ac2
    let mut w = world();
    w.ok("alex", decide(&w, "a", "rejected", json!({})));
    assert_eq!(
        (
            data(&w, "a")["decision"]["actor"].as_str(),
            data(&w, "a")["decision"]["actor_type"].as_str()
        ),
        (Some("alex"), Some("person"))
    );
    w.ok(
        "agent",
        decide(&w, "b", "changed_spec", json!({"links":["ac:ios"]})),
    );
    assert_eq!(
        (
            data(&w, "b")["decision"]["actor"].as_str(),
            data(&w, "b")["decision"]["actor_type"].as_str()
        ),
        (Some("agent"), Some("agent"))
    );
}
