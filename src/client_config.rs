use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{io::Write, path::PathBuf};

#[derive(Default, Deserialize, Serialize)]
pub struct Config {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}
pub fn validate_url(value: &str) -> Result<String> {
    let url = reqwest::Url::parse(value).context("Invalid server URL")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && local),
        "Use HTTPS (HTTP is allowed only on localhost)"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "Server URL must be an origin without credentials, query or path"
    );
    Ok(url.as_str().trim_end_matches('/').into())
}
pub fn path() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("JOPPA_CONFIG") {
        return Ok(p.into());
    }
    Ok(
        PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?)
            .join(".config/joppa/config.json"),
    )
}
pub fn load() -> Result<Config> {
    let p = path()?;
    if !p.exists() {
        return Ok(Config::default());
    }
    let mut c: Config = serde_json::from_slice(&std::fs::read(&p)?)
        .context("Could not read Joppa client config")?;
    c.url = validate_url(&c.url)?;
    Ok(c)
}
pub fn save(c: &Config) -> Result<()> {
    validate_url(&c.url)?;
    let path = path()?;
    let dir = path.parent().context("Invalid config path")?;
    std::fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".joppa-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temp)?;
        file.write_all(serde_json::to_string_pretty(c)?.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_keys_only_go_to_a_secure_origin() {
        assert_eq!(
            validate_url("https://joppa.example/").unwrap(),
            "https://joppa.example"
        );
        assert!(validate_url("http://127.0.0.1:9000").is_ok());
        for url in [
            "http://remote.example",
            "https://user:secret@example.com",
            "https://example.com/path",
            "https://example.com?key=secret",
            "file:///tmp/key",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
    }
}
