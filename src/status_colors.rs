//! Status tones in the tree schema: an ordered list with a light and a dark colour each.
//! The list sets the colours and the order of the status filter. One fixed priority decides
//! every rollup. A schema without `statuses` uses the default list. The older `status_colors`
//! map stays readable for earlier journal events. It colours the default list for both themes,
//! and a saved `statuses` list replaces it. A stored list of an earlier release names the older
//! tone of blue (status_colors_alias.rs).
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[path = "status_colors_alias.rs"]
mod alias;
pub use alias::{filter_tone, stored_colors, stored_statuses};
pub(crate) use alias::{read_colors, read_statuses};

/// The status tones that the status rules produce.
pub const STATUS_TONES: [&str; 6] = ["red", "purple", "yellow", "orange", "blue", "green"];

/// The tone of an archived node. Only a read that includes archived nodes finds it.
pub const ARCHIVED_TONE: &str = "purple";

/// The label of each tone in the status filter of the board.
pub const LABELS: [(&str, &str); 6] = [
    ("red", "Checks failed"),
    ("purple", "Archived"),
    ("yellow", "Waits"),
    ("orange", "No tests"),
    ("blue", "Awaits checks"),
    ("green", "Checks passed"),
];

/// The label of a tone in the status filter of the board.
pub fn label(tone: &str) -> &'static str {
    LABELS
        .iter()
        .find(|(t, _)| *t == tone)
        .map_or("", |(_, l)| l)
}

/// One status tone with its colour in the light and in the dark theme.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub tone: String,
    pub light: String,
    pub dark: String,
}

/// The default list for a workspace without its own.
pub fn defaults() -> Vec<Status> {
    [
        ("red", "#bc5d52", "#bc5d52"),
        ("purple", "#8061a9", "#9b7bc6"),
        ("yellow", "#ae7a16", "#c8921f"),
        ("orange", "#c4772e", "#c4772e"),
        ("blue", "#6fa8dc", "#8bbbe8"),
        ("green", "#39835a", "#39835a"),
    ]
    .into_iter()
    .map(|(tone, light, dark)| Status {
        tone: tone.into(),
        light: light.into(),
        dark: dark.into(),
    })
    .collect()
}

fn hex(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Refuse an unknown tone or a colour that is not #RRGGBB (the older map).
pub fn validate(colors: &BTreeMap<String, String>) -> Result<()> {
    for (tone, color) in colors {
        ensure!(
            STATUS_TONES.contains(&tone.as_str()),
            "status tones are red, purple, yellow, orange, blue and green"
        );
        ensure!(hex(color), "a status colour is #RRGGBB");
    }
    Ok(())
}

/// Refuse an empty list, an unknown or repeated tone, or a colour that is not #RRGGBB.
pub fn validate_statuses(statuses: &[Status]) -> Result<()> {
    ensure!(!statuses.is_empty(), "statuses needs at least one tone");
    let mut seen = BTreeSet::new();
    for s in statuses {
        ensure!(
            STATUS_TONES.contains(&s.tone.as_str()),
            "status tones are red, purple, yellow, orange, blue and green"
        );
        ensure!(
            seen.insert(s.tone.as_str()),
            "each status tone appears once"
        );
        ensure!(hex(&s.light) && hex(&s.dark), "a status colour is #RRGGBB");
    }
    Ok(())
}

/// The list that applies: the schema list, else the default list coloured by the older map.
/// A tone that the schema list omits keeps its default colours at the end of the order.
pub fn effective(
    statuses: Option<&Vec<Status>>,
    colors: Option<&BTreeMap<String, String>>,
) -> Vec<Status> {
    let mut list = statuses.cloned().unwrap_or_else(defaults);
    for d in defaults() {
        if !list.iter().any(|s| s.tone == d.tone) {
            list.push(d);
        }
    }
    let colors = if statuses.is_some() { None } else { colors };
    for (tone, color) in colors.into_iter().flatten() {
        if let Some(s) = list.iter_mut().find(|s| &s.tone == tone) {
            s.light = color.clone();
            s.dark = color.clone();
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(tone: &str, light: &str, dark: &str) -> Status {
        Status {
            tone: tone.into(),
            light: light.into(),
            dark: dark.into(),
        }
    }

    #[test]
    fn every_tone_has_one_filter_label() {
        for tone in STATUS_TONES {
            assert!(!label(tone).is_empty(), "{tone}");
        }
        assert_eq!((label("blue"), label("grey")), ("Awaits checks", ""));
    }

    #[test]
    fn missing_tones_follow_the_saved_list() {
        let list = vec![
            status("green", "#22a559", "#22a559"),
            status("red", "#ff0000", "#ff0000"),
        ];
        let tones: Vec<String> = effective(Some(&list), None)
            .into_iter()
            .map(|s| s.tone)
            .collect();
        assert_eq!(
            tones,
            ["green", "red", "purple", "yellow", "orange", "blue"]
        );
    }

    #[test]
    fn the_older_map_overrides_both_theme_colours() {
        let colors: BTreeMap<String, String> = [("blue".to_owned(), "#c8c3b8".to_owned())].into();
        let blue = effective(None, Some(&colors))
            .into_iter()
            .find(|s| s.tone == "blue")
            .unwrap();
        assert_eq!(
            (blue.light.as_str(), blue.dark.as_str()),
            ("#c8c3b8", "#c8c3b8")
        );
        // A saved list replaces the older map.
        let own = vec![status("blue", "#aaaaaa", "#555555")];
        let blue = effective(Some(&own), Some(&colors))
            .into_iter()
            .find(|s| s.tone == "blue")
            .unwrap();
        assert_eq!(
            (blue.light.as_str(), blue.dark.as_str()),
            ("#aaaaaa", "#555555")
        );
    }

    #[test]
    fn bad_lists_and_colours_are_refused() {
        let bad = [
            (vec![], "at least one"),
            (
                vec![status("grey", "#000000", "#000000")],
                "status tones are red, purple, yellow, orange, blue and green",
            ),
            (
                vec![
                    status("red", "#000000", "#000000"),
                    status("red", "#111111", "#111111"),
                ],
                "once",
            ),
            (vec![status("red", "#00000", "#000000")], "#RRGGBB"),
            (vec![status("red", "#000000", "red")], "#RRGGBB"),
        ];
        for (list, error) in bad {
            let message = validate_statuses(&list).unwrap_err().to_string();
            assert!(message.contains(error), "{list:?}: {message}");
        }
        let colors: BTreeMap<String, String> = [("grey".to_owned(), "#000000".to_owned())].into();
        assert!(validate(&colors).is_err());
    }
}
