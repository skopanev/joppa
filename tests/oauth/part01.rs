// Part 1 of tests/oauth.rs, included there, so the test names stay unchanged.

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn start(url: &str, tokens: &std::path::Path, port: u16) -> Result<Server> {
    let base = format!("http://127.0.0.1:{port}");
    let server = Server(
        Command::new(env!("CARGO_BIN_EXE_joppa"))
            .args(["serve", &port.to_string()])
            .env("DEEPINFRA_API_KEY", "test-only-key")
            .env("JOPPA_EMBEDDING_URL", "http://127.0.0.1:9/embeddings")
            .envs(fake_spaces::start().env())
            .env("JOPPA_DATABASE_URL", url)
            .env("JOPPA_TOKENS_FILE", tokens)
            .env("JOPPA_PUBLIC_URL", &base)
            .env_remove("JOPPA_LOGIN_USERS_FILE")
            .env_remove("JOPPA_GOOGLE_CLIENT_FILE")
            .env_remove("JOPPA_ACCESS_RULES_FILE")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?,
    );
    for _ in 0..100 {
        if reqwest::get(format!("{base}/health"))
            .await
            .is_ok_and(|r| r.status().is_success())
        {
            return Ok(server);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("server did not start")
}

fn form(v: &Value) -> String {
    let mut url = reqwest::Url::parse("http://form.example").unwrap();
    let mut s = url.query_pairs_mut();
    for (k, v) in v.as_object().unwrap() {
        s.append_pair(k, v.as_str().unwrap());
    }
    drop(s);
    url.query().unwrap().to_owned()
}

async fn exchange(http: &reqwest::Client, base: &str, v: &Value) -> Result<reqwest::Response> {
    Ok(http
        .post(format!("{base}/oauth/token"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form(v))
        .send()
        .await?)
}
