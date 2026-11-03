//! A bounded display projection. Session history remains the durable source of evidence.
use super::{
    markdown::{self, Rendered},
    tool_display,
};
use crate::{provider::Call, runtime::Session};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    User,
    Assistant,
    Reasoning,
    Tool,
    Activity,
    Error,
}
impl Kind {
    fn foldable(self) -> bool {
        !matches!(self, Self::User | Self::Assistant)
    }
}
struct Entry {
    kind: Kind,
    title: String,
    body: String,
    preview: Option<String>,
    expanded: bool,
    call: Option<Call>,
}
#[derive(Default)]
pub struct Transcript {
    entries: Vec<Entry>,
    pub revision: usize,
    stream: Option<usize>,
    expand_new: bool,
    omitted: bool,
}
impl Transcript {
    pub fn boundary(&mut self) {
        self.stream = None;
    }
    pub fn push(&mut self, kind: Kind, title: &str, body: &str) {
        self.boundary();
        self.entries.push(Entry {
            kind,
            title: title.into(),
            body: body.into(),
            preview: None,
            expanded: !kind.foldable() || self.expand_new,
            call: None,
        });
        self.changed();
    }
    pub fn delta(&mut self, kind: Kind, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(i) = self.stream.filter(|i| self.entries[*i].kind == kind) {
            self.entries[i].body.push_str(text);
        } else {
            let title = if kind == Kind::Reasoning {
                "Thinking · service-provided"
            } else {
                "Agent"
            };
            self.push(kind, title, text);
            self.stream = Some(self.entries.len() - 1);
        }
        self.changed();
    }
    pub fn tool(&mut self, call: &Call, state: &str, result: Option<&str>) {
        self.boundary();
        let display = tool_display::present(call, state, result);
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| entry.call.as_ref().is_some_and(|c| c.id == call.id))
        {
            entry.title = display.title;
            entry.body = display.body;
            entry.preview = Some(display.preview);
        } else {
            self.push(Kind::Tool, &display.title, &display.body);
            let entry = self.entries.last_mut().unwrap();
            entry.call = Some(call.clone());
            entry.preview = Some(display.preview);
        }
        self.changed();
    }
    pub fn toggle(&mut self, index: usize) {
        if let Some(entry) = self.entries.get_mut(index).filter(|e| e.kind.foldable()) {
            entry.expanded = !entry.expanded;
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn toggle_all(&mut self) {
        self.expand_new = if self.entries.iter().any(|e| e.kind.foldable()) {
            !self
                .entries
                .iter()
                .filter(|e| e.kind.foldable())
                .all(|e| e.expanded)
        } else {
            !self.expand_new
        };
        for entry in self.entries.iter_mut().filter(|e| e.kind.foldable()) {
            entry.expanded = self.expand_new;
        }
        self.revision = self.revision.wrapping_add(1);
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        let mut bytes: usize = self
            .entries
            .iter()
            .map(|e| e.body.len() + e.title.len() + e.preview.as_ref().map_or(0, String::len))
            .sum();
        if bytes > 200_000 {
            while bytes > 150_000 && self.entries.len() > 1 {
                let entry = self.entries.remove(0);
                bytes -= entry.body.len()
                    + entry.title.len()
                    + entry.preview.as_ref().map_or(0, String::len);
                self.stream = self.stream.and_then(|i| i.checked_sub(1));
            }
            if let Some(entry) = self.entries.last_mut() {
                trim_front(&mut entry.body, 150_000);
            }
            self.omitted = true;
        }
    }
    pub fn render(&self, width: usize) -> (Rendered, Vec<(usize, usize)>) {
        let mut rendered = Rendered::default();
        let mut headers = vec![];
        if self.omitted {
            rendered.lines.extend(
                markdown::render(
                    "*Earlier display text omitted; saved session history is unchanged.*",
                    width,
                )
                .lines,
            );
        }
        for (i, entry) in self.entries.iter().enumerate() {
            let color = match entry.kind {
                Kind::User => Color::Cyan,
                Kind::Assistant => Color::Green,
                Kind::Tool => Color::Blue,
                Kind::Reasoning | Kind::Activity => Color::Gray,
                Kind::Error => Color::Red,
            };
            let prefix = if entry.kind.foldable() {
                if entry.expanded { "▾ " } else { "▸ " }
            } else {
                "● "
            };
            let title = ellipsize(&format!("{prefix}{}", entry.title), width);
            if entry.kind.foldable() {
                headers.push((rendered.lines.len(), i));
            }
            rendered.lines.push(Line::styled(
                title,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            if entry.expanded {
                rendered.lines.push(Line::default());
                let content_marker = if entry
                    .call
                    .as_ref()
                    .is_some_and(|c| c.name == "read_document")
                {
                    "\n\nDocument content\n"
                } else if entry.call.as_ref().is_some_and(|c| c.name == "pm") {
                    "\n\nReport content\n"
                } else {
                    ""
                };
                let body = if !content_marker.is_empty()
                    && let Some((metadata, content)) = entry.body.split_once(content_marker)
                {
                    let mut body = diagnostic_text(metadata, width);
                    body.lines.push(Line::default());
                    let content = markdown::render(content, width);
                    body.lines.extend(content.lines);
                    body.links.extend(content.links);
                    body
                } else if matches!(entry.kind, Kind::Error | Kind::Tool) {
                    diagnostic_text(&entry.body, width)
                } else {
                    markdown::render(&entry.body, width)
                };
                rendered.lines.extend(body.lines);
                rendered.links.extend(body.links);
            } else {
                let source = entry.preview.as_deref().unwrap_or(&entry.body);
                // Notices often repeat their heading as the first line.
                let source = source.strip_prefix(&entry.title).unwrap_or(source).trim();
                let body = if matches!(entry.kind, Kind::Error | Kind::Tool) {
                    diagnostic_text(source, width)
                } else {
                    markdown::render(source, width)
                };
                let mut lines: Vec<_> = body
                    .lines
                    .into_iter()
                    .filter(|line| line.width() > 0)
                    .collect();
                let omitted = lines.len() > 2;
                if entry.kind == Kind::Reasoning && omitted {
                    lines.drain(..lines.len() - 2);
                    if let Some(first) = lines.first_mut() {
                        *first = Line::raw(ellipsize(&format!("…{}", first), width));
                    }
                } else {
                    lines.truncate(2);
                    if omitted && let Some(last) = lines.last_mut() {
                        *last = Line::raw(format!(
                            "{}…",
                            one_line(&last.to_string(), width.saturating_sub(1))
                        ));
                    }
                }
                rendered.lines.extend(lines);
            }
            rendered.lines.push(Line::default());
        }
        (rendered, headers)
    }
    pub fn from_session(session: &Session) -> Self {
        let mut transcript = Self::default();
        for (index, item) in session.history.iter().enumerate() {
            if let Some(summary) = session
                .reasoning_summaries
                .get(&index)
                .filter(|s| !s.is_empty())
            {
                transcript.push(Kind::Reasoning, "Thinking · service-provided", summary);
            }
            if item["type"] == "reasoning" {
                let summary = item["summary"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|p| p["type"] == "summary_text")
                    .filter_map(|p| p["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if !summary.is_empty() {
                    transcript.push(
                        Kind::Reasoning,
                        "Thinking · service-provided summary",
                        &summary,
                    );
                }
            }
            let role = item["role"].as_str().unwrap_or("");
            if matches!(role, "user" | "assistant") {
                let mut body = message_text(item);
                for label in crate::chat_images::labels(item) {
                    body.push_str(&format!("\n\n{label}"));
                }
                if !body.is_empty() {
                    transcript.push(
                        if role == "user" {
                            Kind::User
                        } else {
                            Kind::Assistant
                        },
                        if role == "user" { "You" } else { "Agent" },
                        &body,
                    );
                }
            }
            if item["type"] == "function_call" {
                transcript.tool(
                    &Call {
                        id: item["call_id"].as_str().unwrap_or("").into(),
                        name: item["name"].as_str().unwrap_or("").into(),
                        arguments: item["arguments"].as_str().unwrap_or("").into(),
                    },
                    "Requested",
                    None,
                );
            }
            for call in item["tool_calls"].as_array().into_iter().flatten() {
                transcript.tool(
                    &Call {
                        id: call["id"].as_str().unwrap_or("").into(),
                        name: call["function"]["name"].as_str().unwrap_or("").into(),
                        arguments: call["function"]["arguments"].as_str().unwrap_or("").into(),
                    },
                    "Requested",
                    None,
                );
            }
            transcript.apply_result(item);
        }
        transcript.sync_tools(session);
        if let Some(error) = session.error.as_deref().filter(|e| !e.is_empty()) {
            transcript.push(
                Kind::Error,
                &format!(
                    "{:?}: {}",
                    session.status,
                    error.lines().next().unwrap_or("")
                ),
                error,
            );
        }
        transcript.boundary();
        transcript
    }
    fn apply_result(&mut self, item: &Value) {
        let result = if item["role"] == "tool" {
            Some((&item["tool_call_id"], &item["content"]))
        } else if item["type"] == "function_call_output" {
            Some((&item["call_id"], &item["output"]))
        } else {
            None
        };
        if let Some((id, value)) = result {
            let call = self
                .entries
                .iter()
                .rev()
                .filter_map(|e| e.call.as_ref())
                .find(|c| Some(c.id.as_str()) == id.as_str())
                .cloned();
            if let Some(call) = call {
                self.tool(
                    &call,
                    "Result recorded",
                    Some(value.as_str().unwrap_or(&value.to_string())),
                );
            }
        }
    }
    /// Refresh results for already-visible calls after local approval/denial/reconciliation.
    pub fn sync_tools(&mut self, session: &Session) {
        for item in &session.history {
            self.apply_result(item);
        }
        for call in &session.pending {
            self.tool(call, &format!("Pending · {:?}", session.status), None);
        }
    }
}
pub fn message_text(item: &Value) -> String {
    let content = item["content"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            item["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        });
    content
        .split("\n\n[Application source locator:")
        .next()
        .unwrap_or(&content)
        .into()
}
pub fn ellipsize(text: &str, width: usize) -> String {
    let clean = one_line(text, usize::MAX);
    use unicode_width::UnicodeWidthStr;
    if clean.width() <= width {
        clean
    } else if width == 0 {
        String::new()
    } else {
        format!("{}…", one_line(&clean, width - 1))
    }
}
/// Errors are plain diagnostic data. Preserve path punctuation and line breaks,
/// never interpret it as Markdown, links or terminal control sequences.
fn diagnostic_text(text: &str, width: usize) -> Rendered {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    Rendered {
        lines: markdown::wrap(
            clean
                .replace('\t', "    ")
                .lines()
                .map(|s| Line::raw(s.to_owned()))
                .collect(),
            width,
        ),
        links: vec![],
    }
}
pub fn one_line(text: &str, width: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let mut result = String::new();
    let mut used = 0;
    for g in clean.graphemes(true) {
        if used + g.width() > width {
            break;
        }
        result.push_str(g);
        used += g.width();
    }
    result
}
fn trim_front(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut cut = text.len() - limit;
    while !text.is_char_boundary(cut) {
        cut += 1;
    }
    text.drain(..cut);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn visible(t: &Transcript) -> String {
        t.render(80)
            .0
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn separates_streams_and_collapses_only_activity() {
        let mut t = Transcript::default();
        t.push(Kind::User, "You", "Question");
        t.delta(Kind::Reasoning, "SERVICE_SUMMARY");
        t.delta(Kind::Assistant, "**Readable reply**");
        t.tool(
            &Call {
                id: "1".into(),
                name: "read".into(),
                arguments: "{\"path\":\"DETAIL\"}".into(),
            },
            "Running",
            None,
        );
        assert!(visible(&t).contains("Readable reply"));
        assert!(visible(&t).contains("SERVICE_SUMMARY"));
        t.toggle_all();
        assert!(visible(&t).contains("SERVICE_SUMMARY"));
        assert!(visible(&t).contains("DETAIL"));
        t.toggle_all();
        assert!(visible(&t).contains("DETAIL"));
        assert!(visible(&t).contains("Question"));
        t.boundary();
        t.delta(Kind::Assistant, "Next request");
        assert_eq!(t.entries.len(), 5);
    }
    #[test]
    fn display_is_bounded_and_safe_for_unicode_headers() {
        let mut t = Transcript::default();
        t.delta(Kind::Assistant, &"中".repeat(100_000));
        assert!(t.omitted);
        assert!(t.entries[0].body.len() <= 150_000);
        assert_eq!(one_line("\u{1b}\n中文abc", 5), "中文a");
    }
    #[test]
    fn collapsed_entries_use_at_most_three_physical_lines_and_keep_replies_complete() {
        let mut t = Transcript::default();
        t.push(
            Kind::Activity,
            "Long activity heading with 中文",
            "First detail\n\nSecond detail\n\nTHIRD_DETAIL",
        );
        t.delta(Kind::Reasoning, "OLD\n\nMiddle\n\n最近進度\n\nLatest");
        for width in [12, 25, 80] {
            let (rendered, headings) = t.render(width);
            for (i, (start, _)) in headings.iter().enumerate() {
                let end = headings.get(i + 1).map_or(rendered.lines.len(), |h| h.0);
                let lines = &rendered.lines[*start..end];
                assert!(lines.iter().filter(|l| l.width() > 0).count() <= 3);
                assert!(lines.iter().all(|l| l.width() <= width));
            }
            assert!(
                !rendered
                    .lines
                    .iter()
                    .any(|l| l.to_string().contains("THIRD_DETAIL"))
            );
        }
        let collapsed = visible(&t);
        assert!(!collapsed.contains("OLD"));
        assert!(collapsed.contains("Latest"));
        assert!(collapsed.contains('…'));
        t.delta(
            Kind::Assistant,
            "Answer start\n\nAnswer middle\n\nAnswer end",
        );
        assert!(visible(&t).contains("Answer end"));
        t.toggle_all();
        assert!(visible(&t).contains("THIRD_DETAIL"));
        assert!(visible(&t).contains("OLD"));
    }
    #[test]
    fn live_tool_updates_match_replay_and_expanded_reads_render_document_markdown() {
        use serde_json::json;
        let call = Call {
            id: "r1".into(),
            name: "read_document".into(),
            arguments: json!({"path":"notes/[v1]**.md"}).to_string(),
        };
        let result = json!({"path":"notes/[v1]**.md", "sha256":"digest", "content":"# Document\n\n**Readable content**"}).to_string();
        let mut t = Transcript::default();
        t.tool(&call, "Requested", None);
        t.tool(&call, "Running", None);
        t.tool(&call, "Result recorded", Some(&result));
        assert_eq!(t.entries.len(), 1);
        let s: Session = serde_json::from_value(json!({"id":"fixture", "status":"completed", "provider_fingerprint":"fixture", "history":[{"type":"function_call", "call_id":"r1", "name":"read_document", "arguments":call.arguments}, {"type":"function_call_output", "call_id":"r1", "output":result}], "pending":[], "questions":[], "last_output":"", "error":null, "stage":"workflow"})).unwrap();
        let mut replay = Transcript::from_session(&s);
        assert_eq!(visible(&t), visible(&replay));
        t.toggle_all();
        replay.toggle_all();
        assert_eq!(visible(&t), visible(&replay));
        let expanded = visible(&t);
        assert!(expanded.contains("notes/[v1]**.md"));
        assert!(expanded.contains("Readable content"));
        assert!(!expanded.contains("**Readable content**"));
        assert!(!expanded.contains("\"sha256\""));
        let mut reports = Transcript::default();
        reports.tool(&Call { id:"pm1".into(), name:"pm".into(), arguments:json!({"command":"coverage","args":[]}).to_string() }, "Result recorded", Some(&json!({"exit_code":0,"reports":[{"path":"reports/[v1]**.md", "content":"# Coverage\n\n**One uncovered requirement**"}]}).to_string()));
        assert!(visible(&reports).contains("Check requirement coverage"));
        reports.toggle_all();
        let expanded = visible(&reports);
        assert!(expanded.contains("reports/[v1]**.md"));
        assert!(expanded.contains("One uncovered requirement"));
        assert!(!expanded.contains("**One uncovered requirement**"));
    }
    #[test]
    fn replay_restores_saved_failure_details_without_inventing_missing_information() {
        use serde_json::json;
        let error = "Load instructions\nFile: /tmp/**old**/Schema_[v1].md\nCause: No such file or directory (os error 2)\nNext: Check the selected Vault.";
        let s: Session = serde_json::from_value(json!({"id":"fixture", "status":"failed", "provider_fingerprint":"fixture", "history":[], "pending":[], "questions":[], "last_output":"", "error":error, "stage":"workflow"})).unwrap();
        let mut t = Transcript::from_session(&s);
        assert!(visible(&t).contains("Failed"));
        assert!(visible(&t).contains("Schema_[v1].md"));
        assert!(!visible(&t).contains("Check the selected Vault"));
        t.toggle_all();
        let expanded = visible(&t);
        assert!(expanded.contains("/tmp/**old**/Schema_[v1].md"));
        assert!(expanded.contains("os error 2"));
        assert!(expanded.contains("Check the selected Vault"));
        let mut old = s;
        old.error = Some("No such file or directory (os error 2)".into());
        let mut t = Transcript::from_session(&old);
        t.toggle_all();
        assert!(!visible(&t).contains("Schema_[v1].md"));
        assert!(visible(&t).contains("os error 2"));
    }
    #[test]
    fn failure_details_cannot_create_links_or_emit_terminal_controls() {
        let mut t = Transcript::default();
        let details = "File: /tmp/**old**/[notes].md\nCause: \u{1b}[31mblocked\u{0}\n[link](https://example.test)";
        t.push(Kind::Error, "Failed: Read guidance", details);
        let (rendered, _) = t.render(100);
        assert_eq!(rendered.lines[0].style.fg, Some(Color::Red));
        assert!(visible(&t).contains("/tmp/"));
        assert!(rendered.links.is_empty());
        t.toggle_all();
        let (rendered, _) = t.render(100);
        assert!(rendered.links.is_empty());
        let body = rendered
            .lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(body.contains("/tmp/**old**/[notes].md"));
        assert!(body.contains("[link](https://example.test)"));
        assert!(!body.contains('\u{1b}') && !body.contains('\u{0}'));
    }
    #[test]
    fn resume_reconstructs_both_protocols_without_disclosing_opaque_or_inventing_reasoning() {
        use serde_json::json;
        let legacy = json!({"id":"fixture", "status":"completed", "provider_fingerprint":"fixture", "history":[], "pending":[], "questions":[], "last_output":"", "error":null, "stage":"workflow"});
        let mut s: Session = serde_json::from_value(legacy).unwrap();
        assert!(s.reasoning_summaries.is_empty());
        s.history = vec![
            json!({"role":"user","content":"Question\n\n[Application source locator: PRIVATE_LOCATOR]"}),
            json!({"role":"user","content":[{"type":"text","text":"\n\n[Application source locator: PRIVATE_LOCATOR]"},{"type":"pm_image","image":{"width":20,"height":10,"path":"PRIVATE_LOCATOR"}}]}),
            json!({"role":"assistant","content":"Answer", "tool_calls":[{"id":"chat-call","function":{"name":"read","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"chat-call","content":"{\"error\":\"SYNTHETIC_ERROR\"}"}),
            json!({"type":"reasoning","encrypted_content":"OPAQUE_SECRET","summary":[{"type":"summary_text","text":"PUBLIC_SUMMARY"}]}),
            json!({"type":"function_call","call_id":"responses-call","name":"inspect","arguments":"{}"}),
            json!({"type":"function_call_output","call_id":"responses-call","output":"{\"status\":\"blocked_by_clarification\"}"}),
        ];
        s.reasoning_summaries.insert(1, "CHAT_REASONING".into());
        let s: Session = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        let mut t = Transcript::from_session(&s);
        let collapsed = visible(&t);
        assert!(collapsed.contains("Answer"));
        assert!(collapsed.contains("Image 1 · 20×10 PNG"));
        assert!(collapsed.contains("Error reported"));
        assert!(collapsed.contains("Blocked by clarification"));
        assert!(collapsed.contains("PUBLIC_SUMMARY"));
        t.toggle_all();
        let expanded = visible(&t);
        assert!(expanded.contains("CHAT_REASONING"));
        assert!(expanded.contains("PUBLIC_SUMMARY"));
        assert!(expanded.contains("SYNTHETIC_ERROR"));
        assert!(!expanded.contains("OPAQUE_SECRET"));
        assert!(!expanded.contains("PRIVATE_LOCATOR"));
    }
}
