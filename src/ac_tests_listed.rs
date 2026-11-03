//! The test list of a node: before migrate_test_nodes the field tests of its document, or
//! before migrate_test_rules its own history; after it, the names of its test nodes.
use super::*;

/// The test list of a node: its tests, the revision that saved them and their author.
#[derive(Clone, Debug)]
pub struct Listed {
    pub tests: Vec<String>,
    pub revision: u32,
    pub actor: String,
    /// A list before migrate_test_rules that waits for a save after a text change.
    pub draft: bool,
    /// After migrate_test_nodes, the reset boundary of the node of each test, in list order.
    pub boundaries: Option<Vec<Boundary>>,
}

/// The test list of a node after migrate_test_nodes, with the test nodes of each parent: a test
/// node lists its own test, another node the tests of its test-node children.
pub fn listed_node(
    w: &Workspace,
    address: &str,
    nodes: &std::collections::BTreeMap<String, Vec<(String, String)>>,
) -> Option<Listed> {
    let current = w.objects.get(address)?.current();
    let own;
    let nodes = if crate::test_nodes::is_test(w, address) {
        own = vec![(
            address.to_owned(),
            current.data["title"].as_str()?.to_owned(),
        )];
        &own
    } else {
        nodes.get(address)?
    };
    let boundary = |a: &str| Boundary::read(w.criterion_boundaries.get(a).map(String::as_str));
    Some(Listed {
        tests: nodes.iter().map(|(_, name)| name.clone()).collect(),
        revision: current.revision,
        actor: current.actor.clone(),
        draft: false,
        boundaries: Some(nodes.iter().map(|(a, _)| boundary(a)).collect()),
    })
}

/// The test list of a node. After migrate_test_rules it is the field tests of the current
/// document. Before it, the list had its own history and a draft after a text change.
pub fn listed(w: &Workspace, address: &str) -> Option<Listed> {
    let o = w.objects.get(address)?;
    // After migrate_test_nodes, a test node names its one test in its title, and another
    // node lists the tests of its test-node children.
    if w.test_nodes_migrated {
        let names = crate::test_nodes::nodes_by_parent(w);
        return listed_node(w, address, &names);
    }
    if w.test_rules_migrated {
        // A node of a type with the status rule tests always has a list: its document
        // revision is the revision of the list, also before the first test.
        let current = o.current();
        let tests_type = crate::objects::schema(w)
            .types
            .get(&o.kind)
            .is_some_and(|t| crate::objects::status_rule(t) == "tests");
        let tests: Vec<String> = match current.data.get("tests") {
            Some(list) => list
                .as_array()?
                .iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect(),
            None if tests_type => vec![],
            None => return None,
        };
        return Some(Listed {
            tests,
            revision: current.revision,
            actor: current.actor.clone(),
            draft: false,
            boundaries: None,
        });
    }
    let list = w.ac_tests.get(address).and_then(AcTests::current)?;
    let legacy = crate::test_rules::legacy_draft_fields();
    Some(Listed {
        tests: list.tests.clone(),
        revision: list.revision,
        actor: list.actor.clone(),
        draft: list.text_hash != hash_of(&legacy, &o.current().data),
        boundaries: None,
    })
}
