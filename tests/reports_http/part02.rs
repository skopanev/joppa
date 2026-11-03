// Part 2 of tests/reports_http.rs, included there, so the test names stay unchanged.

/// A workspace on the test server: spec:one with criteria a and b.
/// The token member `agent` is P, and `other` is Q.
struct Planning<'a> {
    w: &'a World,
    pool: deadpool_postgres::Pool,
    ws: String,
}
