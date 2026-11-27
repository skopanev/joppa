use super::*;

#[test]
fn a_title_that_matches_the_query_adds_a_fixed_score() {
    let title = "The tools/list reply is at most 13500 bytes";
    assert_eq!(
        title_boost(title, "The Tools/List reply is at most 13500 bytes "),
        0.10
    );
    assert_eq!(title_boost(title, "reply is at most"), 0.035);
    assert_eq!(title_boost(title, "tools/list budget"), 0.025);
    assert_eq!(title_boost(title, "bytes reply most"), 0.025);
    assert_eq!(title_boost(title, "budget limit"), 0.0);
    assert_eq!(title_boost(title, "at"), 0.0);
    // The exact title beats any fused pair of word and vector ranks.
    let pair = 2.0 / 61.0;
    assert!(title_boost(title, title) > pair, "{pair}");
}
