#![allow(dead_code)]

use joppa::{
    commands::{self, Command},
    model::Workspace,
};
use serde_json::{Value, json};
pub struct World {
    pub state: Workspace,
    pub position: u64,
}
impl World {
    pub fn new() -> Self {
        let mut world = Self {
            state: Workspace::default(),
            position: 0,
        };
        // The example product comes from an old journal: its removed operations replay.
        for v in setup() {
            if is_removed(&v) {
                world.legacy("owner", v);
            } else {
                world.ok("owner", v);
            }
        }
        world
    }
    /// One event of an old journal with an operation that live writes no longer accept.
    pub fn legacy(&mut self, actor: &str, value: Value) {
        let cmd: Command =
            serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{value}: {e}"));
        commands::apply_replay(
            &mut self.state,
            actor,
            &cmd,
            self.position + 1,
            "2026-01-01T00:00:00Z",
        )
        .unwrap_or_else(|e| panic!("{value}: {e}"));
        self.position += 1;
    }
    pub fn apply(&mut self, actor: &str, value: Value) -> anyhow::Result<()> {
        let cmd: Command = serde_json::from_value(value)?;
        let mut copy = self.state.clone();
        // Время в тестах предсказуемое: иначе проверки на содержимое
        // комментария зависели бы от часов машины.
        commands::apply(
            &mut copy,
            actor,
            &cmd,
            self.position + 1,
            "2026-01-01T00:00:00Z",
        )?;
        self.state = copy;
        self.position += 1;
        Ok(())
    }
    /// A write that must succeed. This World holds an old journal, so a removed operation
    /// is one of its events and replays; `no` still sends it as a live write.
    pub fn ok(&mut self, actor: &str, v: Value) {
        if is_removed(&v) {
            return self.legacy(actor, v);
        }
        self.apply(actor, v.clone())
            .unwrap_or_else(|e| panic!("{v}: {e}"));
    }
    pub fn no(&mut self, actor: &str, v: Value) {
        assert!(
            self.apply(actor, v.clone()).is_err(),
            "unexpected success: {v}"
        );
    }
    pub fn summary(&self, n: usize) -> Value {
        joppa::checks::summary(&self.state.requirements["r"].revisions[n - 1])
    }
    pub fn ready(&mut self) {
        self.ok("owner", json!({"op":"confirm","req":"r","revision":1}));
        for ac in ["ios", "key"] {
            self.ok("agent", assess(ac, true));
        }
    }
    pub fn run(&mut self, id: &str, outcome: &str) {
        self.ok("runner", plan(id));
        self.ok(
            "runner",
            result(id, "[ios] paid access", outcome, vec!["access retained"]),
        );
        self.ok(
            "owner",
            result(id, "key-permissions", "pass", vec!["read", "verify"]),
        );
        self.ok(
            "runner",
            json!({"op":"finish","req":"r","revision":1,"run":id}),
        );
    }
}
/// An operation that live writes refuse and old journals still hold.
pub fn is_removed(value: &Value) -> bool {
    value["op"]
        .as_str()
        .is_some_and(|op| joppa::removed_ops::NAMES.contains(&op))
}
pub fn setup() -> Vec<Value> {
    vec![
        json!({"op":"workspace","title":"Example product"}),
        json!({"op":"enroll_service","id":"agent","role":"agent"}),
        json!({"op":"enroll_service","id":"runner","role":"runner"}),
        json!({"op":"member","id":"legal","role":"human"}),
        json!({"op":"domain","id":"d","title":"Access","authority":"owner"}),
        json!({"op":"capability","id":"c","title":"Keep paid access","domain":"d"}),
        json!({"op":"consumer","id":"customer","title":"Paying customer","owner":"owner"}),
        json!({"op":"project","id":"app","repository":"https://example.test/app"}),
        json!({"op":"project","id":"backend","repository":"https://example.test/backend"}),
        json!({"op":"project","id":"infra","repository":"https://example.test/infra"}),
        revision(0),
    ]
}
pub fn revision(expected: u32) -> Value {
    json!({"op":"revise","req":"r","expected_revision":expected,"capability":"c","consumers":["customer"],"title":"Paid access persists","body":"Only iOS is in scope","owner":"owner","reason":"Agreed wording","acs":[{"id":"ios","text":"On iOS paid access survives login"},{"id":"key","text":"Before production key has exactly read and verify permissions","before_launch":true}]})
}
pub fn assess(ac: &str, ready: bool) -> Value {
    json!({"op":"assess","req":"r","revision":1,"ac":ac,"assessment":{"ready":ready,"reason":if ready {"Known scope"} else {"Which exact permissions?"},"recipient":if ready {None} else {Some("legal")},"decision":null}})
}
pub fn plan(run: &str) -> Value {
    json!({"op":"plan","req":"r","revision":1,"run":run,"subject":{"app":"sha-a","backend":"sha-b","infra":"config-v1","environment":"staging"},"scope":"iOS access and provider settings","discovery_complete":true,"checks":[{"id":"[ios] paid access","ac":"ios","method":"automated","expected":["access retained"]},{"id":"key-permissions","ac":"key","method":"manual","expected":["read","verify"]}]})
}
pub fn result(run: &str, check: &str, outcome: &str, observed: Vec<&str>) -> Value {
    json!({"op":"result","req":"r","revision":1,"run":run,"evidence":{"check":check,"outcome":outcome,"observed":observed,"note":"Inspected exact result","link":null}})
}

/// One event of an old journal with an operation that live writes refuse: the journal gets
/// it as stored, and the projections rebuild from the journal, as on a server restart.
pub async fn append_legacy(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    request: &str,
    value: &Value,
) -> anyhow::Result<()> {
    pool.get().await?.execute(
        "INSERT INTO joppa.events(workspace,position,actor,request_id,command) SELECT $1,COALESCE(MAX(position),0)+1,$2,$3,$4::text::jsonb FROM joppa.events WHERE workspace=$1",
        &[&ws, &actor, &request, &value.to_string()],
    ).await?;
    joppa::store::backfill_board(pool).await?;
    Ok(())
}

/// Write each value as `actor`: a live operation through the store, a removed one as an
/// event of an old journal.
pub async fn seed(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    values: impl IntoIterator<Item = Value>,
) -> anyhow::Result<()> {
    for value in values {
        let request = uuid::Uuid::new_v4().to_string();
        if is_removed(&value) {
            append_legacy(pool, ws, actor, &request, &value).await?;
        } else {
            joppa::store::execute_input(pool, ws, actor, &request, &value).await?;
        }
    }
    Ok(())
}

/// Save a new title of an object and keep its other fields, parent and links.
pub async fn save_title(
    pool: &deadpool_postgres::Pool,
    ws: &str,
    actor: &str,
    request: &str,
    address: &str,
    title: &str,
    reason: &str,
) -> anyhow::Result<Value> {
    let current = joppa::store::read_objects(
        pool,
        ws,
        actor,
        &joppa::store::ObjectScope {
            address: Some(address),
            ..Default::default()
        },
    )
    .await?;
    let (kind, id) = address.split_once(':').unwrap_or_default();
    let mut data = current["document"]["data"].clone();
    data["title"] = json!(title);
    joppa::store::execute_input(
        pool,
        ws,
        actor,
        request,
        &json!({"op":"save_object","kind":kind,"id":id,"revision":current["current_revision"],
            "parent":current["document"]["parent"],"data":data,"links":current["document"]["links"],"reason":reason}),
    )
    .await
}
