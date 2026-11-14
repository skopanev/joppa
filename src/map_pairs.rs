//! The pairs of two sorted maps in key order, without a lookup for each key. A write reads
//! the maps of the workspace before and after it side by side.
use std::collections::BTreeMap;

/// One key of either map, with its value before and after.
pub struct Pair<'a, T> {
    pub key: &'a str,
    pub old: Option<&'a T>,
    pub new: Option<&'a T>,
}

/// Each key of `old` and `new` once, in key order.
pub fn pairs<'a, T>(
    old: &'a BTreeMap<String, T>,
    new: &'a BTreeMap<String, T>,
) -> impl Iterator<Item = Pair<'a, T>> {
    let (mut old, mut new) = (old.iter().peekable(), new.iter().peekable());
    std::iter::from_fn(move || {
        let order = match (old.peek(), new.peek()) {
            (None, None) => return None,
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some((a, _)), Some((b, _))) => a.as_str().cmp(b.as_str()),
        };
        Some(match order {
            std::cmp::Ordering::Less => {
                let (key, value) = old.next()?;
                Pair {
                    key,
                    old: Some(value),
                    new: None,
                }
            }
            std::cmp::Ordering::Greater => {
                let (key, value) = new.next()?;
                Pair {
                    key,
                    old: None,
                    new: Some(value),
                }
            }
            std::cmp::Ordering::Equal => {
                let (key, before) = old.next()?;
                let after = new.next().map(|(_, v)| v);
                Pair {
                    key,
                    old: Some(before),
                    new: after,
                }
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_key_of_either_map_comes_once_in_key_order() {
        let old = BTreeMap::from([("a".to_owned(), 1), ("c".to_owned(), 3)]);
        let new = BTreeMap::from([("b".to_owned(), 2), ("c".to_owned(), 4)]);
        let found: Vec<_> = pairs(&old, &new)
            .map(|p| (p.key, p.old.copied(), p.new.copied()))
            .collect();
        assert_eq!(
            found,
            [
                ("a", Some(1), None),
                ("b", None, Some(2)),
                ("c", Some(3), Some(4))
            ]
        );
    }
}
