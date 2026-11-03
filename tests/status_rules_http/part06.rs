// Part 6 of tests/status_rules_http.rs, included there, so the test names stay unchanged.

impl World {
    /// The present test nodes directly under a node, as (address, test name).
    async fn test_nodes(&self, address: &str) -> Result<Vec<(String, String)>> {
        let read = json!({"parent":address,"kind":"test","depth":1,"limit":100});
        let list = self.mcp(&self.alex, "joppa_objects", read).await?;
        let items = list["items"].as_array().context("test nodes")?;
        Ok(items
            .iter()
            .filter_map(|i| Some((i["address"].as_str()?.into(), i["title"].as_str()?.into())))
            .collect())
    }
    /// Make the tests of a node exactly `tests`: a new test gets a test node, and a test that
    /// leaves the list loses its node.
    async fn bind(&self, address: &str, tests: &[&str]) -> Result<()> {
        let tests: Vec<String> = tests.iter().map(|t| format!("{SUITE} > {t}")).collect();
        let nodes = self.test_nodes(address).await?;
        for (node, name) in &nodes {
            if !tests.contains(name) {
                self.unbind(node).await?;
            }
        }
        for test in tests.iter().filter(|t| !nodes.iter().any(|(_, n)| n == *t)) {
            self.command(&self.owner, json!({"op":"save_object","kind":"test","revision":0,"parent":address,"data":{"title":test,"owner":"owner"},"reason":"Bind the test"})).await?;
        }
        Ok(())
    }
    /// Remove one test node from its criterion: archive the node.
    async fn unbind(&self, node: &str) -> Result<()> {
        let revision = self.object(node).await?["current_revision"].clone();
        let archive = json!({"op":"archive_object","target":node,"revision":revision,"reason":"Unbind the test"});
        let (code, reply) = self.try_command(&self.owner, archive).await?;
        ensure!(code == 200, "{node}: {code} {reply}");
        Ok(())
    }
    /// Archive a criterion: its test nodes leave it first, because a node with active
    /// children cannot be archived.
    async fn archive(&self, address: &str) -> Result<()> {
        self.bind(address, &[]).await?;
        let revision = self.object(address).await?["current_revision"].clone();
        self.command(&self.owner, json!({"op":"archive_object","target":address,"revision":revision,"reason":"Drop the check"})).await?;
        Ok(())
    }
}
