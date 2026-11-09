//! requirement:check-list-frozen over a real server: a new title, body or file of a node of a
//! source type resets the criteria that name it. A detach, another data value and a change of
//! a document without the flag reset nothing.
#[path = "common/record_world.rs"]
mod record_world;
#[path = "common/source_world.rs"]
mod source_world;
use anyhow::Result;
use serde_json::json;
use source_world::{NAMED, expect, record, save};

// ac:jyzav1zw
// test:TBD
#[tokio::test]
#[ignore = "requires isolated JOPPA_TEST_DATABASE_URL"]
async fn a_new_title_body_or_file_of_a_source_resets_the_criteria_that_name_it() -> Result<()> {
    let w = source_world::world().await?;
    // Each change of S makes A, B, H and K grey. C and E stay green.
    save(&w, "src:s", json!({"body":"The new source text"})).await?;
    expect(&w, "the new body of S", &NAMED).await?;
    record(&w, 1).await?;
    expect(&w, "the record after the body", &[]).await?;
    save(&w, "src:s", json!({"title":"S renamed"})).await?;
    expect(&w, "the new title of S", &NAMED).await?;
    record(&w, 2).await?;
    expect(&w, "the record after the title", &[]).await?;
    let file = source_world::attach(&w).await?;
    expect(&w, "the file on S", &NAMED).await?;
    record(&w, 3).await?;
    expect(&w, "the record after the file", &[]).await?;
    // These changes keep every criterion green.
    source_world::detach(&w, &file).await?;
    expect(&w, "the detach from S", &[]).await?;
    save(&w, "src:s", json!({"env":"dev"})).await?;
    expect(&w, "the new data.env of S", &[]).await?;
    save(&w, "doc:d", json!({"body":"The new document text"})).await?;
    expect(&w, "the new body of D", &[]).await?;
    Ok(())
}
