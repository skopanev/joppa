//! Part of `reports`: the message and the fields of the deciding row of each test. The status
//! reads light rows (store_reports.rs), so only these rows give their texts, in one read.
use super::*;
use std::collections::BTreeMap;

/// The message and the fields of a row, by its report, its stored name and its status.
type Found = BTreeMap<(i64, String, String), (Option<String>, Option<Value>)>;

/// Fill the message and the fields of each test of `statuses` from its deciding row, and remove
/// the row mark that compute() left.
pub(super) async fn fill(
    client: &impl GenericClient,
    workspace: &str,
    statuses: &mut BTreeMap<String, Value>,
    rules: &crate::result_shape::ResultShape,
) -> Result<()> {
    let (mut reports, mut names) = (vec![], vec![]);
    for test in tests(statuses) {
        reports.extend(test["row"]["report"].as_i64());
        let each = test["row"]["names"].as_array().into_iter().flatten();
        names.extend(each.filter_map(Value::as_str).map(str::to_owned));
    }
    let mut found = Found::new();
    if !reports.is_empty() {
        let sql = "SELECT t.report_id,t.address,t.status,t.message,t.fields::text FROM joppa.test_results t JOIN joppa.test_reports r ON r.id=t.report_id WHERE r.workspace=$1 AND t.report_id=ANY($2) AND t.address=ANY($3)";
        for r in client.query(sql, &[&workspace, &reports, &names]).await? {
            let fields = r.get::<_, Option<String>>(4);
            let fields = fields.and_then(|f| serde_json::from_str(&f).ok());
            found.insert((r.get(0), r.get(1), r.get(2)), (r.get(3), fields));
        }
    }
    for test in tests_mut(statuses) {
        let Some(row) = test.as_object_mut().and_then(|t| t.remove("row")) else {
            continue;
        };
        let status = test["status"].as_str().unwrap_or_default().to_owned();
        let report = row["report"].as_i64().unwrap_or_default();
        let names = row["names"].as_array().into_iter().flatten();
        let Some((message, fields)) = names
            .filter_map(Value::as_str)
            .find_map(|name| found.get(&(report, name.to_owned(), status.clone())))
        else {
            continue;
        };
        if let Some(fields) = fields {
            test["fields"] = fields.clone();
        }
        // Older rows keep the message in its own column. Newer rows keep it in the failure field.
        if status == "fail" {
            let field = rules.failure_field.as_deref();
            let from_fields = || fields.as_ref()?.get(field?)?.as_str().map(String::from);
            test["message"] = json!(message.clone().or_else(from_fields));
        }
    }
    Ok(())
}

fn tests(statuses: &BTreeMap<String, Value>) -> impl Iterator<Item = &Value> {
    let each = statuses.values().filter_map(|s| s["tests"].as_array());
    each.flatten().filter(|t| t.get("row").is_some())
}

fn tests_mut(statuses: &mut BTreeMap<String, Value>) -> impl Iterator<Item = &mut Value> {
    let each = statuses
        .values_mut()
        .filter_map(|s| s["tests"].as_array_mut());
    each.flatten()
}
