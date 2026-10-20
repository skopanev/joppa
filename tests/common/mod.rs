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
        for v in setup() {
            world.ok("owner", v);
        }
        world
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
    pub fn ok(&mut self, actor: &str, v: Value) {
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
