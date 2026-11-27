//! User-facing projections of tool data. Original calls/results stay in session history.
use crate::provider::Call;
use serde_json::Value;

pub(super) struct Presentation {
    pub title: String,
    pub preview: String,
    pub body: String,
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

pub(super) fn action(call: &Call) -> String {
    let args = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    match call.name.as_str() {
        "list_documents" => "List project documents".into(),
        "search_documents" => format!("Search documents by keyword “{}”", text(&args, "query")),
        "list_source_files" => {
            if text(&args, "source_id").is_empty() {
                "List registered sources".into()
            } else {
                format!("List source files · {}", text(&args, "source_id"))
            }
        }
        "read_document" => format!("Read document · {}", text(&args, "path")),
        "write_draft" => format!("Save draft · {}", text(&args, "path")),
        "ask_clarification" => format!("Ask for clarification · {}", text(&args, "question")),
        "resolve_clarification" => format!("Resolve clarification · {}", text(&args, "path")),
        "pm" => pm_action(text(&args, "command")).into(),
        _ => format!("External tool · {}", call.name),
    }
}

fn pm_action(command: &str) -> &str {
    match command {
        "validate" => "Validate project documents",
        "coverage" => "Check requirement coverage",
        "plan" => "Calculate effort and capacity",
        "status" => "Review project status",
        "delivery" => "Review delivery readiness",
        "questions" => "Review open questions",
        "materials" => "Inspect source materials",
        "inspect" => "Inspect document references and versions",
        "impact" => "Analyze change impact",
        "fingerprint" => "Check document version fingerprints",
        "settings" => "Inspect or update project settings",
        "ingest" => "Import source material",
        "repo-add" => "Register source repository",
        "sync" => "Capture repository snapshot",
        "evidence" => "Capture source evidence",
        "prepare" => "Prepare an unexecuted work packet",
        "baseline" => "Create a baseline candidate",
        "okf-export" => "Export knowledge bundle",
        "okf-check" => "Check knowledge bundle",
        _ => "Run project operation",
    }
}

pub(super) fn failed(value: &Value) -> bool {
    value.get("error").is_some_and(|v| !v.is_null())
        || value["success"] == false
        || value["successful"] == false
        || value["exit_code"].as_i64().is_some_and(|n| n != 0)
}

fn status(value: &Value) -> String {
    if failed(value) {
        return "Error reported".into();
    }
    match text(value, "status") {
        "draft_saved" => "Draft saved · review required".into(),
        "waiting_for_user" => "Waiting for your answer".into(),
        "blocked_by_clarification" => "Blocked by clarification".into(),
        "blocked_by_grounding" => "Blocked by source support review".into(),
        "" => "Result recorded".into(),
        other => other.replace('_', " "),
    }
}

pub(super) fn present(call: &Call, state: &str, result: Option<&str>) -> Presentation {
    let args = serde_json::from_str(&call.arguments).unwrap_or(Value::Null);
    let value = result.and_then(|s| serde_json::from_str::<Value>(s).ok());
    let outcome = value.as_ref().map(status).unwrap_or_else(|| state.into());
    let mut preview = String::new();
    let mut body = format!("Tool: {}\n\nRequest\n{}", call.name, describe(&args));
    if args.is_null() && !call.arguments.trim().is_empty() {
        body.push_str(
            "\nRequest could not be interpreted. Inspect the saved call before proceeding.",
        );
    }
    if let Some(value) = &value {
        body.push_str("\n\nResult\n");
        match call.name.as_str() {
            "list_documents" | "search_documents" if value.is_array() => {
                let rows = value.as_array().unwrap();
                let count = rows.len();
                let count_label = if call.name == "search_documents" && count >= 60 {
                    format!("{count} documents returned · search limit reached; total unknown")
                } else {
                    format!("{count} documents found")
                };
                preview = count_label.clone();
                if !rows.is_empty() {
                    preview.push('\n');
                    preview.push_str(&names(rows));
                }
                body.push_str(&count_label);
                body.push('\n');
                body.push_str(&describe_result(value));
            }
            "list_source_files" if !failed(value) => {
                let rows = value["files"].as_array().or(value["sources"].as_array());
                if let Some(rows) = rows {
                    preview = format!("{} entries returned", rows.len());
                    if let Some(total) = value["total"].as_u64() {
                        preview.push_str(&format!(" · {total} in selected scope"));
                    }
                    if !value["next_offset"].is_null() {
                        preview.push_str(" · more pages available");
                    }
                    preview.push_str(&format!("\n{}", names(rows)));
                }
                body.push_str("Listing only; file contents have not been read.\n");
                body.push_str(&describe_result(value));
            }
            "read_document" if value["content"].is_string() && !failed(value) => {
                let content = text(value, "content");
                preview = format!(
                    "{} lines read\n{}",
                    content.lines().count(),
                    excerpt(content)
                );
                body.push_str(&format!(
                    "File: {}\nSHA256: {}\n\nDocument content\n{}",
                    text(value, "path"),
                    text(value, "sha256"),
                    content
                ));
            }
            "write_draft" if value["status"] == "draft_saved" => {
                preview = if args["expected_hash"].is_string() {
                    "Draft update saved"
                } else {
                    "New draft saved"
                }
                .into();
                preview.push_str(". Human semantic review is still required.");
                body.push_str(&preview);
                body.push('\n');
                body.push_str(&describe_result(value));
            }
            "ask_clarification" if value["status"] == "waiting_for_user" => {
                preview = format!(
                    "{}\n{}",
                    text(&args, "question"),
                    text(value, "question_path")
                );
                body.push_str(&describe_result(value));
            }
            "resolve_clarification" => {
                preview = text(&args, "resolution").into();
                body.push_str(&describe_result(value));
            }
            "pm" => {
                preview = pm_preview(value);
                let mut details = value.clone();
                let mut reports = vec![];
                if let Some(rows) = details.get_mut("reports").and_then(Value::as_array_mut) {
                    for report in rows {
                        let path = text(report, "path").to_owned();
                        if let Some(fields) = report.as_object_mut()
                            && let Some(Value::String(content)) = fields.remove("content")
                        {
                            reports.push(format!("    Report: {path}\n\n{content}"));
                        }
                    }
                }
                body.push_str(&describe_result(&details));
                if !reports.is_empty() {
                    body.push_str("\n\nReport content\n");
                    body.push_str(&reports.join("\n\n"));
                }
            }
            _ => {
                body.push_str(&describe_result(value));
            }
        }
        if failed(value) {
            preview = value.get("error").map(describe).unwrap_or_else(|| {
                format!(
                    "Exit code: {}\n{}",
                    value["exit_code"],
                    text(value, "stderr")
                )
            });
        } else if text(value, "status").starts_with("blocked_") {
            preview = format!("{}\n{}", status(value), text(value, "question_path"));
        }
        if preview.trim().is_empty() {
            preview = describe_result(value);
        }
    } else if let Some(result) = result {
        // Legacy and external text results are still readable, without a JSON envelope.
        preview = excerpt(result);
        body.push_str(&format!("\n\nResult\n{result}"));
    } else {
        preview = describe(&args);
    }
    bound(&mut body, 120_000);
    bound(&mut preview, 2_000);
    let needs_attention = state == "Approval required"
        || state.starts_with("Stopped")
        || value.as_ref().is_some_and(|v| {
            failed(v)
                || text(v, "status").starts_with("blocked_")
                || v["status"] == "waiting_for_user"
        });
    Presentation {
        title: if needs_attention {
            format!("{outcome} · {}", action(call))
        } else {
            format!("{} · {outcome}", action(call))
        },
        preview,
        body,
    }
}

fn names(rows: &[Value]) -> String {
    let labels = rows
        .iter()
        .take(3)
        .map(|v| {
            ["title", "source_path", "path", "id"]
                .iter()
                .find_map(|key| v[key].as_str())
                .unwrap_or("Unnamed entry")
        })
        .collect::<Vec<_>>()
        .join(" · ");
    if rows.len() > 3 {
        format!("{labels} · +{} more", rows.len() - 3)
    } else {
        labels
    }
}

fn excerpt(content: &str) -> String {
    content
        .lines()
        .filter(|s| !s.trim().is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join("\n")
}

fn pm_preview(value: &Value) -> String {
    let mut lines = vec![];
    if let Some(counts) = value.get("counts") {
        lines.push(describe(counts));
    }
    if let Some(reports) = value["reports"].as_array() {
        for report in reports.iter().take(2) {
            lines.push(format!("Report: {}", text(report, "path")));
        }
    }
    if lines.is_empty() {
        let stdout = text(value, "stdout");
        if !stdout.trim().is_empty() {
            lines.push(excerpt(stdout));
        }
    }
    lines.join("\n")
}

/// Plain field/value layout, including all approval arguments. No data is interpreted
/// as terminal instructions or Markdown. Bounded to protect the display projection.
pub(super) fn describe(value: &Value) -> String {
    describe_data(value, false)
}
fn describe_result(value: &Value) -> String {
    describe_data(value, true)
}
fn describe_data(value: &Value, decode_text: bool) -> String {
    fn visit(value: &Value, depth: usize, out: &mut String, decode_text: bool) {
        if out.len() > 120_000 {
            return;
        }
        if depth > 12 {
            out.push_str("Further nested details are available in saved session history.\n");
            return;
        }
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if out.len() > 120_000 {
                        break;
                    }
                    out.push_str(&"  ".repeat(depth));
                    let label = key.replace('_', " ");
                    out.push_str(&label);
                    out.push(':');
                    if value.is_object() || value.is_array() {
                        out.push('\n');
                        visit(value, depth + 1, out, decode_text);
                    } else {
                        out.push(' ');
                        visit(value, depth + 1, out, decode_text);
                    }
                }
                if fields.is_empty() {
                    out.push_str("None\n");
                }
            }
            Value::Array(rows) => {
                if rows.is_empty() {
                    out.push_str("No entries\n");
                }
                for (i, row) in rows.iter().enumerate() {
                    if out.len() > 120_000 {
                        break;
                    }
                    out.push_str(&format!("{}{}. ", "  ".repeat(depth), i + 1));
                    if row.is_object() || row.is_array() {
                        out.push('\n');
                    }
                    visit(row, depth + 1, out, decode_text);
                }
            }
            Value::String(s) => {
                if decode_text
                    && let Ok(nested) = serde_json::from_str::<Value>(s)
                    && (nested.is_object() || nested.is_array())
                {
                    out.push('\n');
                    visit(&nested, depth + 1, out, decode_text);
                } else {
                    let mut s = s.clone();
                    bound(&mut s, 120_000);
                    out.push_str(&s);
                    out.push('\n');
                }
            }
            Value::Null => out.push_str("null (not provided)\n"),
            Value::Bool(b) => out.push_str(if *b { "true\n" } else { "false\n" }),
            Value::Number(n) => out.push_str(&format!("{n}\n")),
        }
    }
    let mut out = String::new();
    visit(value, 0, &mut out, decode_text);
    bound(&mut out, 120_000);
    out.trim_end().into()
}

fn bound(text: &mut String, limit: usize) {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n… Display shortened; complete data remains in saved session history.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, args: Value) -> Call {
        Call {
            id: "fixture".into(),
            name: name.into(),
            arguments: args.to_string(),
        }
    }

    #[test]
    fn every_builtin_has_a_specific_action_and_readable_result() {
        for spec in crate::tools::specs() {
            let c = call(
                &spec.name,
                json!({"query":"付款", "path":"notes/[v1]**.md", "command":"validate", "question":"Which date?"}),
            );
            let p = present(
                &c,
                "Running",
                Some("{\"status\":\"recorded\",\"detail\":\"Evidence retained\"}"),
            );
            assert!(!p.title.starts_with("External tool"), "{}", spec.name);
            assert!(p.body.contains("Evidence retained"));
            assert!(!p.body.contains("{\"status\""));
        }
        assert!(
            action(&call("resolve_clarification", json!({"path":"Q1.md"})))
                .starts_with("Resolve clarification")
        );
        for command in crate::tools::specs()
            .into_iter()
            .find(|s| s.name == "pm")
            .unwrap()
            .parameters["properties"]["command"]["enum"]
            .as_array()
            .unwrap()
        {
            assert_ne!(
                pm_action(command.as_str().unwrap()),
                "Run project operation"
            );
        }
    }

    #[test]
    fn search_counts_do_not_claim_a_total_when_the_tool_limit_is_reached() {
        let c = call("search_documents", json!({"query":"付款"}));
        let p = present(
            &c,
            "Running",
            Some(&json!([{"title":"付款流程", "path":"付款.md"},{"title":"Refunds"}]).to_string()),
        );
        assert!(p.title.contains("付款"));
        assert!(p.preview.contains("2 documents found"));
        assert!(p.preview.contains("付款流程"));
        let rows = vec![json!({"title":"Match"}); 60];
        let p = present(&c, "Running", Some(&json!(rows).to_string()));
        assert!(p.preview.contains("total unknown"));
        assert!(p.preview.contains("+57 more"));
        assert!(
            present(&c, "Running", Some("[]"))
                .preview
                .contains("0 documents found")
        );
    }

    #[test]
    fn projections_keep_failures_pagination_and_review_requirements_visible() {
        let p = present(
            &call("pm", json!({"command":"validate"})),
            "Running",
            Some("{\"successful\":false,\"exit_code\":2,\"stderr\":\"Missing file\"}"),
        );
        assert!(p.title.contains("Error reported"));
        assert!(p.preview.contains("Missing file"));
        let p = present(
            &call("list_source_files", json!({"source_id":"S1"})),
            "Running",
            Some("{\"files\":[{\"path\":\"a.md\"}],\"total\":50,\"next_offset\":1}"),
        );
        assert!(p.preview.contains("more pages"));
        assert!(p.body.contains("contents have not been read"));
        let p = present(
            &call("write_draft", json!({"path":"draft.md"})),
            "Running",
            Some("{\"status\":\"draft_saved\"}"),
        );
        assert!(p.title.contains("review required"));
        assert!(p.preview.contains("Human semantic review"));
    }

    #[test]
    fn approval_arguments_and_legacy_results_are_readable_without_json() {
        let c = call(
            "mcp_docs_update",
            json!({"path":"**[file]**.md", "enabled":false, "ids":["A", "B"]}),
        );
        let p = present(&c, "Approval required", None);
        assert!(p.body.contains("**[file]**.md"));
        assert!(p.body.contains("enabled: false"));
        assert!(p.body.contains("A") && p.body.contains("B"));
        assert!(!p.body.contains("{\""));
        assert!(
            present(&c, "Result recorded", Some("Legacy text response"))
                .body
                .contains("Legacy text response")
        );
        // Unexpected historical result shapes still have a readable fallback.
        for result in ["null", "[]", "\"Legacy text\"", "{}"] {
            present(
                &call("pm", json!({"command":"status"})),
                "Result recorded",
                Some(result),
            );
        }
    }
}
