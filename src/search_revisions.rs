//! One search row for each object revision, for a search with all_versions. A revision is
//! immutable, except that a redaction rewrites it, so a changed text gets a new hash and
//! loses its old vector. The content hash covers the same text as the embedding input: the
//! first 8000 characters of the title, a line break and the content.
use super::*;

const PROJECT: &str = "INSERT INTO joppa.search_revisions(workspace,address,kind,local_id,revision,title,content,content_hash)
  SELECT x.workspace,x.address,x.kind,x.local_id,x.revision,x.title,x.content,
    encode(sha256(convert_to(left(x.title||E'\\n'||x.content,8000),'UTF8')),'hex')
  FROM (SELECT r.workspace,r.address,split_part(r.address,':',1) AS kind,
      substr(r.address,strpos(r.address,':')+1) AS local_id,r.revision,
      COALESCE(r.document->'data'->>'title',substr(r.address,strpos(r.address,':')+1)) AS title,
      left(COALESCE(r.document->'data','{}'::jsonb)::text,60000) AS content
    FROM joppa.object_revisions r
    WHERE r.workspace=$1 AND ($2::text[] IS NULL OR r.address=ANY($2))) x
  ON CONFLICT(workspace,address,revision) DO UPDATE SET title=EXCLUDED.title,content=EXCLUDED.content,
    content_hash=EXCLUDED.content_hash,embedding=NULL,embedding_profile=NULL,attempts=0,last_error=NULL
  WHERE search_revisions.content_hash<>EXCLUDED.content_hash OR search_revisions.title<>EXCLUDED.title";

/// Add the missing revision rows of the selected objects, or of all objects when
/// `selected` is None, and renew each row whose text changed.
pub(super) async fn project(
    client: &impl GenericClient,
    workspace: &str,
    selected: Option<&std::collections::BTreeSet<String>>,
) -> Result<()> {
    let selected = selected.map(|s| s.iter().cloned().collect::<Vec<_>>());
    client.execute(PROJECT, &[&workspace, &selected]).await?;
    Ok(())
}
