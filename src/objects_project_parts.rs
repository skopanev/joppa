//! Part of `objects_project`: the reads of one step and the writes of the pending rows.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tokio_postgres::types::ToSql;

const DOCS: &str = "INSERT INTO joppa.objects(workspace,address,kind,local_id,revision,parent,present,title,owner,description,document_at,inbox_status,document,signal,process) SELECT $1,x.address,x.kind,x.local_id,x.revision,x.parent,x.present,x.title,x.owner,x.description,x.document_at,x.inbox_status,x.document,x.signal,x.process FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,kind text,local_id text,revision integer,parent text,present boolean,title text,owner text,description text,document_at text,inbox_status text,document jsonb,signal jsonb,process jsonb) ON CONFLICT(workspace,address) DO UPDATE SET revision=EXCLUDED.revision,parent=EXCLUDED.parent,present=EXCLUDED.present,title=EXCLUDED.title,owner=EXCLUDED.owner,description=EXCLUDED.description,document_at=EXCLUDED.document_at,inbox_status=EXCLUDED.inbox_status,document=EXCLUDED.document,signal=EXCLUDED.signal,process=EXCLUDED.process";
const CHANGED: &str = "SELECT x.document::text,r.document::text FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,revision integer,document jsonb) JOIN joppa.object_revisions r ON r.workspace=$1 AND r.address=x.address AND r.revision=x.revision WHERE r.document IS DISTINCT FROM x.document";
const REVISIONS: &str = "INSERT INTO joppa.object_revisions(workspace,address,revision,document) SELECT $1,x.address,x.revision,x.document FROM jsonb_to_recordset($2::text::jsonb) AS x(address text,revision integer,document jsonb) ON CONFLICT(workspace,address,revision) DO NOTHING";
const LINKS: &str = "INSERT INTO joppa.object_links(workspace,source,target,relation,revision,when_label) SELECT $1,x.source,x.target,x.relation,x.revision,x.when_label FROM jsonb_to_recordset($2::text::jsonb) AS x(source text,target text,relation text,revision integer,when_label text)";
const IDS: &str = "SELECT joppa.register_item($1,x.kind,x.local_id,$3) FROM jsonb_to_recordset($2::text::jsonb) AS x(kind text,local_id text)";

/// The rows that wait for their write.
#[derive(Default)]
pub(super) struct Pending {
    pub(super) docs: Vec<Value>,
    pub(super) revisions: Vec<Value>,
    pub(super) links: Vec<Value>,
    pub(super) ids: Vec<Value>,
}

/// The next part of `rows`: PART rows, or with `all` the rest.
fn part(rows: &mut Vec<Value>, all: bool) -> Option<String> {
    let count = match rows.len() {
        n if n >= PART => PART,
        n if all && n > 0 => n,
        _ => return None,
    };
    Some(json!(rows.drain(..count).collect::<Vec<_>>()).to_string())
}

impl Pending {
    /// Write each full part, and with `all` the rest. `links` allows the link rows: their old
    /// rows left already.
    pub(super) async fn flush(
        &mut self,
        client: &impl GenericClient,
        workspace: &str,
        prefix: &str,
        links: bool,
        all: bool,
    ) -> Result<()> {
        while let Some(body) = part(&mut self.docs, all) {
            client.execute(DOCS, &[&workspace, &body]).await?;
        }
        // A row stored before migration 062 holds links: it compares by the replay rule.
        while let Some(body) = part(&mut self.revisions, all) {
            each(client, CHANGED, &[&workspace, &body], |row| {
                let replayed: Value = serde_json::from_str(row.get(0))?;
                let stored: Value = serde_json::from_str(row.get(1))?;
                ensure!(
                    stored.get("links").is_some()
                        && crate::relations::adapt_document(stored)? == replayed,
                    "stored object revision differs from the journal"
                );
                Ok(())
            })
            .await?;
            client.execute(REVISIONS, &[&workspace, &body]).await?;
        }
        while links && let Some(body) = part(&mut self.links, all) {
            client.execute(LINKS, &[&workspace, &body]).await?;
        }
        while let Some(body) = part(&mut self.ids, all) {
            client.execute(IDS, &[&workspace, &body, &prefix]).await?;
        }
        Ok(())
    }
}

/// The current projection rows of the objects of one step.
pub(super) async fn existing_of(
    client: &impl GenericClient,
    workspace: &str,
    step: &[&String],
) -> Result<BTreeMap<String, Value>> {
    let mut existing = BTreeMap::new();
    let sql = "SELECT address,jsonb_build_object('address',address,'kind',kind,'local_id',local_id,'revision',revision,'parent',parent,'present',present,'title',title,'owner',owner,'description',description,'document_at',document_at,'inbox_status',inbox_status,'document',document,'signal',signal,'process',process)::text FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)";
    each(client, sql, &[&workspace, &step], |row| {
        existing.insert(row.get(0), serde_json::from_str(row.get(1))?);
        Ok(())
    })
    .await?;
    Ok(existing)
}

/// The registered item IDs of the objects of one step.
pub(super) async fn registered_of(
    client: &impl GenericClient,
    workspace: &str,
    prefix: &str,
    step: &[&String],
) -> Result<BTreeSet<(String, String)>> {
    let mut registered = BTreeSet::new();
    let sql = "SELECT kind,local_id FROM joppa.item_ids WHERE workspace=$1 AND namespace=$2 AND (kind,local_id) IN (SELECT split_part(a,':',1),substr(a,strpos(a,':')+1) FROM unnest($3::text[]) a)";
    each(client, sql, &[&workspace, &prefix, &step], |r| {
        registered.insert((r.get(0), r.get(1)));
        Ok(())
    })
    .await?;
    Ok(registered)
}

/// The objects of `keys` whose current revision is newer than their stored row: their links
/// change. An object without a row is new.
pub(super) async fn stale_sources(
    client: &impl GenericClient,
    workspace: &str,
    state: &Workspace,
    keys: &[&String],
) -> Result<Vec<String>> {
    let mut stored = BTreeMap::new();
    let sql = "SELECT address,revision FROM joppa.objects WHERE workspace=$1 AND address=ANY($2)";
    each(client, sql, &[&workspace, &keys], |row| {
        stored.insert(row.get::<_, String>(0), row.get::<_, i32>(1) as u32);
        Ok(())
    })
    .await?;
    let newer = |key: &&&String| {
        state.objects[**key].current().revision > stored.get(**key).copied().unwrap_or(0)
    };
    Ok(keys
        .iter()
        .filter(newer)
        .map(|key| (*key).clone())
        .collect())
}

/// Remove the links of `sources`, in one statement.
pub(super) async fn delete_links<S: ToSql + Sync>(
    client: &impl GenericClient,
    workspace: &str,
    sources: &[S],
) -> Result<()> {
    if !sources.is_empty() {
        let sql = "DELETE FROM joppa.object_links WHERE workspace=$1 AND source=ANY($2)";
        client.execute(sql, &[&workspace, &sources]).await?;
    }
    Ok(())
}

/// Rewrite the stored revisions of each redacted object.
pub(super) async fn redact(
    client: &impl GenericClient,
    workspace: &str,
    redactions: &[(String, String)],
) -> Result<()> {
    if !redactions.is_empty() {
        // A redaction only removes text, so the field length rule of migration 037 skips it.
        client
            .execute("SELECT set_config('joppa.replaying','1',true)", &[])
            .await?;
    }
    for (address, fragment) in redactions {
        // The fragment as it appears inside a JSON string.
        let encoded = serde_json::to_string(fragment)?;
        let encoded = &encoded[1..encoded.len() - 1];
        client.execute("UPDATE joppa.object_revisions SET document=replace(document::text,$3,$4)::jsonb WHERE workspace=$1 AND address=$2 AND strpos(document::text,$3)>0", &[&workspace,address,&encoded,&crate::redact::MASK]).await?;
    }
    Ok(())
}
