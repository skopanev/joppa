//! Part of `relations`: the colour of a relation key and of an edge. A colour is one name of
//! the palette of Flow view or `#rrggbb`. A save reads `#rrggbb` in lower case, so the journal,
//! the replay and every read keep one form. Only a live save checks the form (check_limits).

/// The palette names. Flow view maps each name to the CSS token --flow-<name>.
pub const PALETTE: [&str; 8] = [
    "blue", "purple", "teal", "amber", "pink", "indigo", "cyan", "lime",
];
/// The refusal of a colour of any other form, also of a colour that is not a string.
pub const COLOR_FORM: &str =
    "color is blue, purple, teal, amber, pink, indigo, cyan, lime or #rrggbb";

/// `#` and 6 hex digits, in either case.
fn hex(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Read the colour of a save. A null is no colour, and a value that is not a string gets the
/// colour refusal, not a type error. A `#rrggbb` colour reads in lower case. Any other string
/// stays as sent, so a live save refuses it with the same text.
pub(crate) fn color<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    use serde::Deserialize;
    use serde::de::Error;
    match Option::<serde_json::Value>::deserialize(d)? {
        None => Ok(None),
        Some(serde_json::Value::String(c)) if hex(&c) => Ok(Some(c.to_ascii_lowercase())),
        Some(serde_json::Value::String(c)) => Ok(Some(c)),
        Some(_) => Err(D::Error::custom(COLOR_FORM)),
    }
}

/// No colour, a palette name or `#rrggbb` in lower case.
pub fn color_form(color: &Option<String>) -> bool {
    color
        .as_deref()
        .is_none_or(|c| PALETTE.contains(&c) || hex(c) && c == c.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::super::{Edge, Patch, Relation};
    use super::*;
    use serde_json::json;

    fn read(value: serde_json::Value) -> Result<Relation, String> {
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    #[test]
    fn a_hex_colour_reads_in_lower_case_and_a_name_stays_as_sent() {
        let r = read(json!({"color":"#1A2B3C","edges":[{"to":"a:b","color":"#AA0000"}]}));
        let r = r.unwrap();
        assert_eq!(r.color.as_deref(), Some("#1a2b3c"));
        assert_eq!(r.edges[0].color.as_deref(), Some("#aa0000"));
        assert_eq!(
            read(json!({"color":"Teal"})).unwrap().color.as_deref(),
            Some("Teal")
        );
    }

    #[test]
    fn a_colour_that_is_not_a_string_gets_the_colour_refusal() {
        assert_eq!(read(json!({"color":7})), Err(COLOR_FORM.to_owned()));
        let edge = read(json!({"edges":[{"to":"a:b","color":true}]}));
        assert!(edge.unwrap_err().starts_with(COLOR_FORM));
        let patch: Result<Patch, _> = serde_json::from_value(json!({"swap":{"color":[1]}}));
        assert!(patch.unwrap_err().to_string().starts_with(COLOR_FORM));
    }

    #[test]
    fn the_form_takes_the_palette_and_lower_case_hex_only() {
        for good in ["teal", "lime", "#1a2b3c"] {
            assert!(color_form(&Some(good.into())), "{good}");
        }
        for bad in ["red", "Teal", "teal ", "#12345", "#1A2B3C", "#gggggg", ""] {
            assert!(!color_form(&Some(bad.into())), "{bad}");
        }
        assert!(color_form(&None));
    }

    #[test]
    fn a_relation_without_colour_and_facts_writes_neither_key() {
        let r = read(json!({"edges":[{"to":"a:b"}]})).unwrap();
        let edge = Edge {
            to: "a:b".into(),
            when: None,
            revision: None,
            color: None,
        };
        assert_eq!(r.edges, [edge]);
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"edges":[{"to":"a:b"}]})
        );
        let r = read(json!({"edges":[],"facts":["a:c"],"color":"pink"})).unwrap();
        let back = serde_json::to_value(&r).unwrap();
        assert_eq!(back, json!({"edges":[],"facts":["a:c"],"color":"pink"}));
    }
}
