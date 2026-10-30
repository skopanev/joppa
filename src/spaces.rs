//! Attachment bytes live in DigitalOcean Spaces (S3), under `files/<sha256>`. The journal
//! keeps only the attachment record: name, type, size and hash.
use crate::spaces_sign::{Signer, encode};
use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::{StatusCode, header};
use std::{sync::OnceLock, time::Duration};

/// The largest attachment: 50 MiB.
pub const ATTACHMENT_MAX: usize = 50 * 1024 * 1024;
/// The largest request body of /mcp and of an upload: 50 MiB in base64 is 66.7 MiB,
/// and the JSON around it fits in the rest.
pub const REQUEST_MAX: usize = 68 * 1024 * 1024;
/// A presigned PUT or GET is valid 15 minutes, a HEAD or DELETE 1 minute.
pub const TRANSFER_TTL: u32 = 900;
pub const CHECK_TTL: u32 = 60;

pub struct Spaces {
    http: reqwest::Client,
    scheme: String,
    host: String,
    bucket: String,
    region: String,
    key_id: String,
    secret: String,
}

impl Spaces {
    /// The variables of /etc/joppa/storage.env. The server does not start without them.
    pub fn from_env() -> Result<Self> {
        let var = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .with_context(|| {
                    format!("set {name}: attachments live in Spaces (server/storage.env.example)")
                })
        };
        Self::new(
            &var("SPACES_ENDPOINT")?,
            &var("SPACES_BUCKET")?,
            &var("SPACES_REGION")?,
            &var("AWS_ACCESS_KEY_ID")?,
            &var("AWS_SECRET_ACCESS_KEY")?,
        )
    }
    /// Plain HTTP is only for a local test endpoint.
    pub fn new(
        endpoint: &str,
        bucket: &str,
        region: &str,
        key_id: &str,
        secret: &str,
    ) -> Result<Self> {
        let url = reqwest::Url::parse(endpoint)?;
        let host = url.host_str().context("the Spaces endpoint has no host")?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http" && matches!(host, "127.0.0.1" | "localhost" | "[::1]")),
            "the Spaces endpoint must use HTTPS"
        );
        ensure!(
            url.path() == "/" && url.query().is_none(),
            "the Spaces endpoint is a bare origin"
        );
        ensure!(
            (3..=63).contains(&bucket.len())
                && bucket.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'
                    || b == b'.'),
            "the Spaces bucket name is invalid"
        );
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(300))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            scheme: url.scheme().into(),
            host: match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host.into(),
            },
            bucket: bucket.into(),
            region: region.into(),
            key_id: key_id.into(),
            secret: secret.into(),
        })
    }
    /// A presigned URL of the file `hash`, path style: `<endpoint>/<bucket>/files/<hash>`.
    pub fn url(&self, method: &str, hash: &str, ttl: u32, extra: &[(&str, &str)]) -> String {
        let path = format!("/{}/files/{hash}", self.bucket);
        let time = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let signer = Signer {
            key_id: &self.key_id,
            secret: &self.secret,
            region: &self.region,
        };
        let query = signer.query(method, &self.host, &path, &time, ttl, extra);
        format!(
            "{}://{}{}?{query}",
            self.scheme,
            self.host,
            encode(&path, true)
        )
    }
    /// A GET URL that gives the file with its own name and content type.
    pub fn download_url(&self, hash: &str, name: &str, content_type: &str) -> String {
        let disposition = format!("inline; filename*=UTF-8''{}", encode(name, false));
        self.url(
            "GET",
            hash,
            TRANSFER_TTL,
            &[
                ("response-content-disposition", &disposition),
                ("response-content-type", content_type),
            ],
        )
    }
    /// The stored size of `hash`, or None when Spaces has no such file.
    pub async fn size(&self, hash: &str) -> Result<Option<u64>> {
        let response = self
            .http
            .head(self.url("HEAD", hash, CHECK_TTL, &[]))
            .send()
            .await
            .map_err(|e| e.without_url())
            .context("file storage unavailable")?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => Ok(response
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok()?.parse().ok())),
            status => bail!("file storage refused HEAD: HTTP {status}"),
        }
    }
    /// Store `bytes` under their SHA-256 hash and return the hash and the size. The same
    /// bytes give the same key, so a second upload changes nothing.
    pub async fn put(&self, bytes: &[u8]) -> Result<(String, usize)> {
        ensure!(!bytes.is_empty(), "an empty file carries nothing");
        ensure!(
            bytes.len() <= ATTACHMENT_MAX,
            "an attachment is at most 50 MiB"
        );
        let hash = crate::digest_hex::sha256_hex(bytes);
        let size = Some(bytes.len() as u64);
        // A key without list rights can get 403 for a missing file: then the PUT decides.
        if self.size(&hash).await.ok().flatten() == size {
            return Ok((hash, bytes.len()));
        }
        let response = self
            .http
            .put(self.url("PUT", &hash, TRANSFER_TTL, &[]))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|e| e.without_url())
            .context("file storage unavailable")?;
        ensure!(
            response.status().is_success(),
            "file storage refused PUT: HTTP {}",
            response.status()
        );
        ensure!(
            self.size(&hash).await? == size,
            "file storage did not keep the file"
        );
        Ok((hash, bytes.len()))
    }
    /// Remove the file `hash`. A file that is not there is already removed.
    pub async fn delete(&self, hash: &str) -> Result<()> {
        let response = self
            .http
            .delete(self.url("DELETE", hash, CHECK_TTL, &[]))
            .send()
            .await
            .map_err(|e| e.without_url())
            .context("file storage unavailable")?;
        ensure!(
            response.status().is_success() || response.status() == StatusCode::NOT_FOUND,
            "file storage refused DELETE: HTTP {}",
            response.status()
        );
        Ok(())
    }
}

static STORAGE: OnceLock<Spaces> = OnceLock::new();

/// Set the file storage of this process, one time.
pub fn configure(spaces: Spaces) -> Result<()> {
    STORAGE
        .set(spaces)
        .map_err(|_| anyhow!("file storage is already configured"))
}

pub fn storage() -> Result<&'static Spaces> {
    STORAGE.get().context("file storage is not configured")
}
