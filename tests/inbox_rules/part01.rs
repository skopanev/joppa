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

