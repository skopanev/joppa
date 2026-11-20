//! Part of `ac_tests_status`: the reset boundary of a criterion.

/// The latest change at or above a criterion, from `Workspace::criterion_boundaries`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// No change reset the criterion: every report counts.
    None,
    /// Only reports received after this time, in microseconds, count.
    After(i64),
    /// The stored boundary cannot be read, so no report counts.
    Unreadable,
}

impl Boundary {
    /// Read a stored journal `recorded_at` time.
    pub fn read(stored: Option<&str>) -> Self {
        let Some(at) = stored else {
            return Self::None;
        };
        chrono::DateTime::parse_from_rfc3339(at)
            .or_else(|_| chrono::DateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S%.f%#z"))
            .map_or(Self::Unreadable, |time| {
                Self::After(time.timestamp_micros())
            })
    }
}
