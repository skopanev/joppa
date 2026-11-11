use super::*;

fn save(kind: &str, id: &str) -> Value {
    json!({"op":"save_object","kind":kind,"id":id,"revision":1})
}

#[test]
fn the_id_kind_and_local_part_gives_the_local_part_of_the_same_kind() {
    let mut value = save("ac", "ac:x");
    local_id(&mut value).unwrap();
    assert_eq!(value["id"], "x");
    // An empty local part and a second ":" stay, so the save refuses them as an invalid id.
    for (id, kept) in [("ac:", ""), ("ac:x:y", "x:y")] {
        let mut value = save("ac", id);
        local_id(&mut value).unwrap();
        assert_eq!(value["id"], kept);
    }
    // A plain local id and a public item ID stay as sent.
    for id in ["x", "joppa-R-DP6KXM62"] {
        let mut value = save("ac", id);
        local_id(&mut value).unwrap();
        assert_eq!(value["id"], id);
    }
}

#[test]
fn the_id_of_another_kind_is_refused() {
    let mut value = save("requirement", "ac:x");
    let error = local_id(&mut value).unwrap_err().to_string();
    assert_eq!(error, "the id names an object of type ac, not requirement");
    assert_eq!(value["id"], "ac:x");
    // Only save_object reads its id.
    let mut note = json!({"op":"note","target":"ac:x","id":"ac:x","text":"t"});
    local_id(&mut note).unwrap();
    assert_eq!(note["id"], "ac:x");
}
