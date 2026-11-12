//! The counts of the objects route: ?count=true gives {total}, and ?group_by=kind,status
//! gives {total, groups}, as joppa_objects gives them (src/node_counts.rs).
use super::*;

pub(super) async fn read(
    pool: &deadpool_postgres::Pool,
    workspace: &str,
    actor: &str,
    q: &ObjectQuery,
) -> anyhow::Result<Value> {
    let mut args = serde_json::Map::new();
    for (key, value) in [("count", &q.count), ("group_by", &q.group_by)] {
        if let Some(value) = value {
            args.insert(key.into(), Value::from(value.as_str()));
        }
    }
    let texts = [
        ("address", q.address.is_some()),
        ("cursor", q.cursor.is_some()),
        ("limit", q.limit.is_some()),
        ("fields", q.fields.is_some()),
        ("relations_of", q.relations_of.is_some()),
        ("in_relation", q.in_relation.is_some()),
        ("related_to", q.related_to.is_some()),
        ("note_status", q.note_status.is_some()),
        ("revision", q.revision.is_some()),
        ("history", q.history.is_some()),
        ("direction", q.direction.is_some()),
        ("relation", q.relation.is_some()),
    ];
    anyhow::ensure!(q.work_filter.is_none(), joppa::removed_ops::WORK_FILTER);
    let sent = texts.iter().filter(|(_, on)| *on).map(|(key, _)| *key);
    let aggregate = joppa::node_counts::Aggregate::parse(&Value::Object(args), sent)?
        .ok_or_else(|| anyhow::anyhow!("count must be true, or group_by must name 1 to 3 keys"))?;
    store::count_objects(pool, workspace, actor, &q.filter()?, &aggregate).await
}
