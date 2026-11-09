//! The older tone grey. An earlier release stored grey for the tone that is now blue. The
//! replay of a journal event and the load of a stored schema revision read grey as blue. Only
//! a stored list takes grey: the parse of a live set_tree_schema keeps it, so set_schema
//! refuses it. The read filters take grey as blue. Migration 063 moves the stored rows of the
//! current state by the same rules.
use super::{Status, defaults};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;

/// The older name of the tone blue.
const OLD_TONE: &str = "grey";
/// The new name.
const TONE: &str = "blue";
/// The default colours of grey in the earlier releases, light and dark.
const OLD_PAIRS: [(&str, &str); 2] = [("#b8b3a8", "#b4b8bf"), ("#c8c3b8", "#8a8f96")];

/// The tone of a read filter: grey is blue, and other values stay.
pub fn filter_tone(tone: &str) -> &str {
    if tone == OLD_TONE { TONE } else { tone }
}

/// A stored status list as this release reads it. A grey entry becomes blue, and its colours
/// become the default colours of blue when they are an older default pair. Other colours stay.
pub fn stored_statuses(list: Vec<Status>) -> Vec<Status> {
    let blue = defaults().into_iter().find(|s| s.tone == TONE);
    list.into_iter()
        .map(|s| match &blue {
            Some(blue) if s.tone == OLD_TONE => {
                let older = OLD_PAIRS.contains(&(s.light.as_str(), s.dark.as_str()));
                Status {
                    tone: TONE.into(),
                    light: if older { blue.light.clone() } else { s.light },
                    dark: if older { blue.dark.clone() } else { s.dark },
                }
            }
            _ => s,
        })
        .collect()
}

/// The older status_colors map as this release reads it: the key grey becomes blue, and each
/// colour stays.
pub fn stored_colors(map: BTreeMap<String, String>) -> BTreeMap<String, String> {
    map.into_iter()
        .map(|(tone, color)| (filter_tone(&tone).to_owned(), color))
        .collect()
}

/// The `statuses` of a stored schema revision, read by stored_statuses.
pub(crate) fn read_statuses<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<Vec<Status>>, D::Error> {
    Ok(Option::<Vec<Status>>::deserialize(d)?.map(stored_statuses))
}

/// The `status_colors` of a stored schema revision, read by stored_colors.
pub(crate) fn read_colors<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<BTreeMap<String, String>>, D::Error> {
    Ok(Option::<BTreeMap<String, String>>::deserialize(d)?.map(stored_colors))
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
    fn a_stored_grey_reads_as_blue() {
        let stored = vec![
            status("red", "#b8b3a8", "#b4b8bf"),
            status("grey", "#b8b3a8", "#b4b8bf"),
        ];
        let new = status("blue", "#6fa8dc", "#8bbbe8");
        let read = stored_statuses(stored);
        assert_eq!(read, [status("red", "#b8b3a8", "#b4b8bf"), new.clone()]);
        let other = stored_statuses(vec![status("grey", "#c8c3b8", "#8a8f96")]);
        assert_eq!(other, [new]);
        // Only an older default pair changes: one colour of it, or a blue entry, stays.
        let kept = [
            status("grey", "#aaaaaa", "#555555"),
            status("grey", "#b8b3a8", "#8a8f96"),
        ];
        for s in kept {
            let read = stored_statuses(vec![s.clone()]);
            assert_eq!(read, [status("blue", &s.light, &s.dark)]);
        }
        let blue = vec![status("blue", "#b8b3a8", "#b4b8bf")];
        assert_eq!(stored_statuses(blue.clone()), blue);
    }

    #[test]
    fn the_older_map_keeps_each_colour() {
        let map = [("grey", "#c8c3b8"), ("red", "#123456")]
            .map(|(t, c)| (t.to_owned(), c.to_owned()))
            .into();
        let read = stored_colors(map);
        let tones: Vec<(&str, &str)> = read.iter().map(|(t, c)| (&**t, &**c)).collect();
        assert_eq!(tones, [("blue", "#c8c3b8"), ("red", "#123456")]);
    }

    #[test]
    fn a_read_filter_takes_grey_as_blue() {
        let tones = ["grey", "blue", "gray", "red"].map(filter_tone);
        assert_eq!(tones, ["blue", "blue", "gray", "red"]);
    }
}
