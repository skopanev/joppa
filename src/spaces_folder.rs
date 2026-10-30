//! The folder of one workspace in Spaces: list its files with ListObjectsV2 and remove them.
use super::*;

/// The text of each `<tag>` element in an S3 XML reply, with the five XML entities decoded.
fn elements(xml: &str, tag: &str) -> Vec<String> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    xml.split(&open)
        .skip(1)
        .filter_map(|part| part.split_once(&close).map(|(text, _)| text))
        .map(|text| {
            text.replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'")
                .replace("&amp;", "&")
        })
        .collect()
}

impl Spaces {
    /// The keys under `prefix`, in pages of at most 1000 that S3 gives.
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query = vec![("list-type", "2"), ("prefix", prefix)];
            if let Some(token) = &token {
                query.push(("continuation-token", token));
            }
            let response = send(self.http.get(self.presign("GET", "", CHECK_TTL, &query))).await?;
            ensure!(
                response.status().is_success(),
                "file storage refused the list: HTTP {}",
                response.status()
            );
            let xml = response.text().await.map_err(|e| e.without_url())?;
            keys.extend(elements(&xml, "Key"));
            if elements(&xml, "IsTruncated").first().map(String::as_str) != Some("true") {
                return Ok(keys);
            }
            token = Some(
                elements(&xml, "NextContinuationToken")
                    .pop()
                    .context("a truncated list has no continuation token")?,
            );
        }
    }

    /// Remove the folder `workspaces/<workspace>/` with every file in it. The folder of
    /// another workspace stays. Returns the number of removed files.
    pub async fn delete_folder(&self, workspace: &str) -> Result<usize> {
        let prefix = Self::folder(workspace)?;
        let keys = self.list(&prefix).await?;
        for key in &keys {
            ensure!(
                key.starts_with(&prefix),
                "the list gave a key outside the folder: {key}"
            );
            self.delete(key).await?;
        }
        Ok(keys.len())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_list_reply_gives_keys_and_the_continuation_token() {
        let xml = "<ListBucketResult><IsTruncated>true</IsTruncated><Contents><Key>workspaces/w/a</Key></Contents><Contents><Key>workspaces/w/b&amp;c</Key></Contents><NextContinuationToken>t+1=</NextContinuationToken></ListBucketResult>";
        assert_eq!(
            super::elements(xml, "Key"),
            ["workspaces/w/a", "workspaces/w/b&c"]
        );
        assert_eq!(super::elements(xml, "IsTruncated"), ["true"]);
        assert_eq!(super::elements(xml, "NextContinuationToken"), ["t+1="]);
    }
}
