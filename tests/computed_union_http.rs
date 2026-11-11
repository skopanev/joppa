//! requirement:8yyywrrq: a list field or a multiple reference with `from` children gives the
//! sorted union of the values of the present children over a real server. Each write
//! recomputes the reached values, a read and the data filter give them, and a save refuses a
//! sent value.
#[path = "common/computed_world.rs"]
mod computed_world;
#[path = "common/record_world.rs"]
mod record_world;
use anyhow::{Result, ensure};
use computed_world::{archive, expect, node, refused, save, schema, sources};
use serde_json::{Value, json};

// ac:axn6d3dp
// test:iqbjcrq1
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn computed_sources_climb_from_the_criteria_as_a_sorted_union() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let group = |extra: Value| node("Capabilities", "group", &[], extra);
    for (rule, error) in [
        (
            json!({"from":"parent"}),
            "sources: a reference takes from children only",
        ),
        (
            json!({"from":"children","multiple":false}),
            "sources: only a reference with multiple: true",
        ),
        (
            json!({"from":"children","required":true}),
            "sources: a reference from children cannot be required",
        ),
    ] {
        let mut refs = sources(Some("children"));
        for (k, v) in rule.as_object().into_iter().flatten() {
            refs["references"]["sources"][k] = v.clone();
        }
        refused(schema(&w, json!({"capability":group(refs)})).await?, error)?;
    }
    // R stays a root until it moves under C.
    let mut requirement = sources(Some("children"));
    requirement["root"] = json!(true);
    let types = json!({"source":node("Sources", "document", &[], json!({"status":"none"})),
        "capability":group(sources(Some("children"))),
        "requirement":node("Requirements", "requirement", &["capability"], requirement),
        "ac":node("Criteria", "ac", &["requirement"], sources(None))});
    schema(&w, types).await?.map_err(anyhow::Error::msg)?;
    for (address, revision, extra) in [
        ("source:s1", Some(0), json!({})),
        ("source:s2", Some(0), json!({})),
        ("source:s3", Some(0), json!({})),
        ("capability:c", Some(0), json!({})),
        ("requirement:r", None, json!({"parent":"capability:c"})),
        // A names local IDs and B full addresses: the union gives addresses.
        ("ac:a", None, json!({"sources":["s2","s1"]})),
        (
            "ac:b",
            Some(0),
            json!({"parent":"requirement:r","sources":["source:s2","source:s3"]}),
        ),
    ] {
        // None saves at the current revision: a move of R revises its criteria too.
        let revision = match revision {
            Some(revision) => revision,
            None => w.object(address).await?["revision"].as_u64().unwrap_or(0),
        };
        save(&w, address, revision, extra)
            .await?
            .map_err(|e| anyhow::anyhow!("{address}: {e}"))?;
    }
    let all = json!(["source:s1", "source:s2", "source:s3"]);
    let wanted = [
        ("requirement:r", all.clone()),
        ("capability:c", all),
        ("ac:a", json!(["s2", "s1"])),
    ];
    let filter = (
        json!(["source:s3"]),
        &["ac:b", "requirement:r", "capability:c"][..],
    );
    expect(&w, "sources", &wanted, filter).await?;
    // A read with fields ["references"] gives the title of each computed address, as for an
    // own reference.
    let named = json!({"address":"capability:c","fields":["references"]});
    let read = w.mcp(&w.bot, "joppa_objects", named).await?;
    let read = read.map_err(anyhow::Error::msg)?;
    let titles: Vec<(&str, &str)> = read["references"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            (
                r["field"].as_str().unwrap_or(""),
                r["title"].as_str().unwrap_or(""),
            )
        })
        .collect();
    ensure!(
        titles == [("sources", "s1"), ("sources", "s2"), ("sources", "s3")],
        "{read}"
    );
    let r = w.object("requirement:r").await?["revision"].as_u64();
    let reply = save(
        &w,
        "requirement:r",
        r.unwrap_or(0),
        json!({"sources":["source:s1"]}),
    )
    .await?;
    refused(
        reply,
        "sources is computed from children and cannot be set on requirement",
    )?;
    archive(&w, "ac:b", 1).await?;
    let two = json!(["source:s1", "source:s2"]);
    let wanted = [("requirement:r", two.clone()), ("capability:c", two)];
    expect(&w, "sources", &wanted, (json!(["source:s3"]), &[])).await?;
    let filter = (json!(["source:s1"]), &["requirement:r", "capability:c"][..]);
    expect(&w, "sources", &[], filter).await
}

// ac:06dfyczp
// test:d8skvjwx
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn computed_tags_give_the_sorted_union_of_the_tasks() -> Result<()> {
    let w = record_world::world(&[]).await?;
    let tags = |from: &str| json!({"fields":{"tags":{"type":"list","from":from}}});
    for (field, error) in [
        (
            json!({"type":"text","from":"children"}),
            "tags: only an enum field takes its value from children or links, and a list field from children",
        ),
        (
            json!({"type":"list","from":"links"}),
            "tags: only an enum field takes its value from children or links",
        ),
    ] {
        let ac = node(
            "Criteria",
            "ac",
            &["requirement"],
            json!({"fields":{"tags":field}}),
        );
        refused(schema(&w, json!({"ac":ac})).await?, error)?;
    }
    let types = json!({"task":node("Tasks", "document", &["ac"], tags("own")),
        "ac":node("Criteria", "ac", &["requirement"], tags("children"))});
    schema(&w, types).await?.map_err(anyhow::Error::msg)?;
    for (address, tags) in [
        ("task:t1", json!(["web", "api"])),
        ("task:t2", json!(["api", "ios"])),
    ] {
        let extra = json!({"parent":"ac:a","tags":tags});
        save(&w, address, 0, extra)
            .await?
            .map_err(anyhow::Error::msg)?;
    }
    let all = json!(["api", "ios", "web"]);
    expect(
        &w,
        "tags",
        &[("ac:a", all.clone())],
        (json!(["web"]), &["task:t1", "ac:a"]),
    )
    .await?;
    save(&w, "task:t2", 1, json!({"tags":["ios"]}))
        .await?
        .map_err(anyhow::Error::msg)?;
    expect(
        &w,
        "tags",
        &[("ac:a", all)],
        (json!(["api"]), &["task:t1", "ac:a"]),
    )
    .await?;
    archive(&w, "task:t1", 1).await?;
    expect(
        &w,
        "tags",
        &[("ac:a", json!(["ios"]))],
        (json!(["web"]), &[]),
    )
    .await?;
    let reply = save(&w, "ac:a", 1, json!({"tags":["web"]})).await?;
    refused(
        reply,
        "tags is computed from children and cannot be set on ac",
    )
}
