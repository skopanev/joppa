//! Part of `reads`: the query of the objects route, moved from web_reads.rs. The node filters
//! have the same form as in joppa_objects.
use super::*;

#[derive(Deserialize)]
pub(in crate::web) struct ObjectQuery {
    pub(in crate::web) address: Option<String>,
    pub(in crate::web) parent: Option<String>,
    pub(in crate::web) kind: Option<String>,
    pub(in crate::web) revision: Option<u32>,
    pub(in crate::web) history: Option<bool>,
    pub(in crate::web) limit: Option<usize>,
    pub(in crate::web) cursor: Option<String>,
    pub(in crate::web) note_status: Option<String>,
    pub(in crate::web) related_to: Option<String>,
    /// Refused: work_filter was removed.
    pub(in crate::web) work_filter: Option<String>,
    /// Refused: links_of was removed.
    pub(in crate::web) links_of: Option<String>,
    /// The address whose edges to read in pages of 25, with optional direction, relation
    /// and kind.
    pub(in crate::web) relations_of: Option<String>,
    pub(in crate::web) direction: Option<String>,
    pub(in crate::web) relation: Option<String>,
    /// The relation key whose members to read in pages of 25 (src/relation_members.rs).
    pub(in crate::web) in_relation: Option<String>,
    /// Comma-separated keys to keep in each list item. The address always stays.
    pub(in crate::web) fields: Option<String>,
    pub(in crate::web) depth: Option<String>,
    pub(in crate::web) status: Option<String>,
    pub(in crate::web) owner: Option<String>,
    pub(in crate::web) assignee: Option<String>,
    /// Refused: the ready filter was removed.
    pub(in crate::web) ready: Option<String>,
    pub(in crate::web) data: Option<String>,
    /// true keeps the blocked nodes, false the other nodes (dependency_sql.rs).
    pub(in crate::web) blocked: Option<String>,
    /// The order of the list, as the order of joppa_objects (dependency_order.rs).
    pub(in crate::web) order: Option<String>,
    /// true gives only the total of the list (src/node_counts.rs).
    pub(in crate::web) count: Option<String>,
    /// 1 to 3 comma-separated keys of the groups of the list.
    pub(in crate::web) group_by: Option<String>,
}
impl ObjectQuery {
    /// The node filters, in the same form as joppa_objects.
    pub(in crate::web) fn filter(&self) -> anyhow::Result<joppa::node_filter::NodeFilter> {
        joppa::node_filter::NodeFilter::parse_query(&[
            ("parent", self.parent.as_deref()),
            ("depth", self.depth.as_deref()),
            ("kind", self.kind.as_deref()),
            ("status", self.status.as_deref()),
            ("owner", self.owner.as_deref()),
            ("assignee", self.assignee.as_deref()),
            ("ready", self.ready.as_deref()),
            ("data", self.data.as_deref()),
            ("blocked", self.blocked.as_deref()),
        ])
    }
    /// The names of the keys that the request sends, outside fields, limit and cursor.
    pub(in crate::web) fn other_keys(&self) -> Vec<&'static str> {
        let texts = [
            ("address", &self.address),
            ("parent", &self.parent),
            ("kind", &self.kind),
            ("note_status", &self.note_status),
            ("related_to", &self.related_to),
            ("work_filter", &self.work_filter),
            ("links_of", &self.links_of),
            ("relations_of", &self.relations_of),
            ("direction", &self.direction),
            ("relation", &self.relation),
            ("depth", &self.depth),
            ("status", &self.status),
            ("owner", &self.owner),
            ("assignee", &self.assignee),
            ("ready", &self.ready),
            ("data", &self.data),
            ("blocked", &self.blocked),
        ];
        let mut keys: Vec<&'static str> = texts
            .into_iter()
            .filter_map(|(key, value)| value.is_some().then_some(key))
            .collect();
        keys.extend(self.revision.map(|_| "revision"));
        keys.extend(self.history.map(|_| "history"));
        keys
    }
}

impl ObjectQuery {
    /// The order of the list. A count, an edge read, a member read and a read by address
    /// refuse it, as joppa_objects does.
    pub(in crate::web) fn list_order(
        &self,
    ) -> anyhow::Result<Option<joppa::dependency_order::Order>> {
        let args = serde_json::json!({"order": self.order, "address": self.address,
            "relations_of": self.relations_of, "in_relation": self.in_relation,
            "group_by": self.group_by, "count": self.count.is_some()});
        joppa::dependency_order::from_args(&args)
    }
}
