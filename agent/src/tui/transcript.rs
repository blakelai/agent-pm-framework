//! A bounded display projection. Session history remains the durable source of evidence.
use super::markdown::{self, Rendered};
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
        let title = format!(
            "Tool · {} · {}",
            call.name,
            result.map(result_label).unwrap_or_else(|| state.into())
        );
        let body = format!(
            "Arguments\n\n{}{}",
            literal(&call.arguments),
            result
                .map(|s| format!("\n\nResult\n\n{}", literal(s)))
                .unwrap_or_default()
        );
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| entry.call.as_ref().is_some_and(|c| c.id == call.id))
        {
            entry.title = title;
            entry.body = body;
        } else {
            self.push(Kind::Tool, &title, &body);
            self.entries.last_mut().unwrap().call = Some(call.clone());
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
            .map(|e| e.body.len() + e.title.len())
            .sum();
        if bytes > 200_000 {
            while bytes > 150_000 && self.entries.len() > 1 {
                let entry = self.entries.remove(0);
                bytes -= entry.body.len() + entry.title.len();
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
            };
            let prefix = if entry.kind.foldable() {
                if entry.expanded { "▾ " } else { "▸ " }
            } else {
                "● "
            };
            let title = one_line(&format!("{prefix}{}", entry.title), width);
            if entry.kind.foldable() {
                headers.push((rendered.lines.len(), i));
            }
            rendered.lines.push(Line::styled(
                title,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            if entry.expanded {
                rendered.lines.push(Line::default());
                let body = markdown::render(&entry.body, width);
                rendered.lines.extend(body.lines);
                rendered.links.extend(body.links);
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
fn result_label(result: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(result) else {
        return "Result recorded".into();
    };
    if value.get("error").is_some()
        || value["success"] == false
        || value["exit_code"].as_i64().is_some_and(|n| n != 0)
    {
        "Error reported".into()
    } else {
        value["status"].as_str().unwrap_or("Result recorded").into()
    }
}
fn literal(text: &str) -> String {
    let text = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| text.into());
    text.lines()
        .map(|s| format!("    {s}"))
        .collect::<Vec<_>>()
        .join("\n")
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
        assert!(!visible(&t).contains("SERVICE_SUMMARY"));
        t.toggle_all();
        assert!(visible(&t).contains("SERVICE_SUMMARY"));
        assert!(visible(&t).contains("DETAIL"));
        t.toggle_all();
        assert!(!visible(&t).contains("DETAIL"));
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
        assert!(collapsed.contains("blocked_by_clarification"));
        assert!(!collapsed.contains("PUBLIC_SUMMARY"));
        t.toggle_all();
        let expanded = visible(&t);
        assert!(expanded.contains("CHAT_REASONING"));
        assert!(expanded.contains("PUBLIC_SUMMARY"));
        assert!(expanded.contains("SYNTHETIC_ERROR"));
        assert!(!expanded.contains("OPAQUE_SECRET"));
        assert!(!expanded.contains("PRIVATE_LOCATOR"));
    }
}
