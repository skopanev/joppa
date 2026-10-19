//! The colours of the status dots, set in the tree schema next to the status rules.
//! A schema without status_colors keeps the board colours. A schema change that omits
//! status_colors keeps the colours of the previous schema revision.
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

/// The status tones that a schema can colour.
pub const STATUS_TONES: [&str; 6] = ["green", "grey", "red", "yellow", "orange", "purple"];

/// Refuse an unknown tone or a colour that is not #RRGGBB.
pub fn validate(colors: &BTreeMap<String, String>) -> Result<()> {
    for (tone, color) in colors {
        ensure!(
            STATUS_TONES.contains(&tone.as_str()),
            "status_colors keys are green, grey, red, yellow, orange and purple"
        );
        ensure!(
            color.len() == 7
                && color.starts_with('#')
                && color[1..].chars().all(|c| c.is_ascii_hexdigit()),
            "a status colour is #RRGGBB"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate;
    use std::collections::BTreeMap;

    fn colors(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn known_tones_with_hex_colours_pass() {
        validate(&colors(&[("green", "#22a559"), ("grey", "#C8C3B8")])).unwrap();
    }

    #[test]
    fn unknown_tones_and_bad_colours_are_refused() {
        for (pairs, error) in [
            (colors(&[("blue", "#000000")]), "keys are green"),
            (colors(&[("green", "22a559")]), "#RRGGBB"),
            (colors(&[("green", "#22a5")]), "#RRGGBB"),
            (colors(&[("grey", "#gggggg")]), "#RRGGBB"),
            (colors(&[("grey", "red; x")]), "#RRGGBB"),
        ] {
            let message = validate(&pairs).unwrap_err().to_string();
            assert!(message.contains(error), "{pairs:?}: {message}");
        }
    }
}
