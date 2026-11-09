//! The member read of one relation (in_relation on the object list). A member of relation K
//! is a present node that holds K, also with no edges, or the present target of a K edge of a
//! present node. The read gives the members in address order, in pages of at most 25, with
//! a bare address as the cursor. It takes no other filter.
use anyhow::{Result, ensure};

/// The refusal of a member read with any other list key.
pub const ONLY: &str = "in_relation takes only fields, limit and cursor";

/// The keys that a member read takes. The workspace and the agent alias name the caller.
pub const KEYS: [&str; 6] = [
    "workspace",
    "alias",
    "in_relation",
    "fields",
    "limit",
    "cursor",
];

/// The largest page of a member read. A larger limit gives this page.
pub const PAGE: usize = 25;

/// Refuse a member read that sends a key outside `KEYS`.
pub fn check_keys<'a>(sent: impl IntoIterator<Item = &'a str>) -> Result<()> {
    for key in sent {
        ensure!(KEYS.contains(&key), ONLY);
    }
    Ok(())
}

/// The page size of a member read: 25 without a limit, and at most 25 with one.
pub fn page(limit: Option<usize>) -> usize {
    limit.map_or(PAGE, |n| n.min(PAGE))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ac:49128yzr
    #[test]
    fn a_member_read_takes_only_fields_limit_and_cursor() {
        assert!(
            check_keys([
                "workspace",
                "alias",
                "in_relation",
                "fields",
                "limit",
                "cursor"
            ])
            .is_ok()
        );
        for key in ["parent", "address", "related_to", "status", "kind", "data"] {
            let refusal = check_keys(["in_relation", key]).unwrap_err().to_string();
            assert_eq!(refusal, ONLY, "{key}");
        }
    }

    // ac:49128yzr
    #[test]
    fn a_member_read_gives_pages_of_at_most_25() {
        assert_eq!(page(None), 25);
        assert_eq!(page(Some(100)), 25);
        assert_eq!(page(Some(10)), 10);
    }
}
