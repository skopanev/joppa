//! JUnit XML to test results. The address is every enclosing testsuite name
//! plus the testcase name, joined by " > ". Bun names its outer suite after the
//! file, so a Bun test reads "file > describe > name".
use super::SEPARATOR;
use anyhow::{Context, Result};
use quick_xml::{Reader, events::BytesStart, events::Event};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestResult {
    pub address: String,
    /// One of pass, fail, skipped, todo.
    pub status: String,
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
}

fn attribute(element: &BytesStart, name: &str) -> Result<Option<String>> {
    match element.try_get_attribute(name)? {
        Some(value) => Ok(Some(
            value
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .context("bad attribute in the JUnit report")?
                .into_owned(),
        )),
        None => Ok(None),
    }
}

fn testcase(suites: &[String], element: &BytesStart) -> Result<TestResult> {
    let name = attribute(element, "name")?.context("a testcase has no name")?;
    // The outer suite is the file: keep it relative, with "/" and no leading "./".
    let mut parts: Vec<String> = suites.iter().filter(|s| !s.is_empty()).cloned().collect();
    if let Some(file) = parts.first_mut() {
        *file = file.replace('\\', "/").trim_start_matches("./").to_owned();
    }
    parts.push(name);
    let duration_ms = attribute(element, "time")?
        .and_then(|t| t.parse::<f64>().ok())
        .map(|seconds| (seconds * 1000.0).round() as i64);
    Ok(TestResult {
        address: parts.join(SEPARATOR),
        status: "pass".into(),
        duration_ms,
        message: None,
    })
}

fn outcome(result: &mut TestResult, element: &BytesStart) -> Result<()> {
    match element.name().as_ref() {
        "failure" | "error" => {
            result.status = "fail".into();
            result.message = attribute(element, "message")?;
        }
        "skipped" => {
            let todo = attribute(element, "message")?.is_some_and(|m| m == "TODO");
            result.status = if todo { "todo" } else { "skipped" }.into();
        }
        _ => {}
    }
    Ok(())
}

pub fn parse_junit(xml: &str) -> Result<Vec<TestResult>> {
    let mut reader = Reader::from_str(xml);
    let mut suites: Vec<String> = vec![];
    let mut open: Option<TestResult> = None;
    let mut results = vec![];
    loop {
        match reader
            .read_event()
            .context("the JUnit report is not valid XML")?
        {
            Event::Start(e) => match e.name().as_ref() {
                "testsuite" => suites.push(attribute(&e, "name")?.unwrap_or_default()),
                "testcase" => open = Some(testcase(&suites, &e)?),
                _ => {
                    if let Some(result) = open.as_mut() {
                        outcome(result, &e)?;
                    }
                }
            },
            Event::Empty(e) => match e.name().as_ref() {
                "testcase" => results.push(testcase(&suites, &e)?),
                _ => {
                    if let Some(result) = open.as_mut() {
                        outcome(result, &e)?;
                    }
                }
            },
            Event::End(e) => match e.name().as_ref() {
                "testsuite" => {
                    suites.pop();
                }
                "testcase" => results.extend(open.take()),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(results)
}
