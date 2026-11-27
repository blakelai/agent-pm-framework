//! Local composer suggestions. Selecting a row inserts text; it never executes a command or Skill.
use super::editor::Editor;
use crate::skills::Catalog;
use ratatui::layout::Rect;
use std::ops::Range;

pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show controls and commands"),
    ("/status", "Inspect this session and reported usage"),
    ("/budget", "Inspect reported session usage and token guard"),
    ("/skills", "Choose a project Skill"),
    ("/skills clear", "Clear additional Skills for this session"),
    ("/language", "Choose the response language for new sessions"),
    ("/stage", "Choose a workflow stage for a new session"),
    ("/search", "Filter project documents"),
    ("/pm", "Run a native project-management command"),
    ("/probe", "Explicitly test the configured provider"),
    ("/probe last", "Read the last saved provider observation"),
    ("/sessions", "List saved sessions"),
    ("/resume", "Resume a saved session ID"),
    ("/continue", "Continue a paused session"),
    (
        "/continue-independent",
        "Continue only supported independent work",
    ),
    ("/answer", "Answer a recorded question: ID TEXT"),
    ("/approve", "Approve the exact pending external call"),
    ("/deny", "Deny the pending call and stop the response"),
    (
        "/reconcile",
        "Record the actual outcome of an interrupted call",
    ),
    ("/new", "Start a fresh session"),
    (
        "/exit",
        "Exit; active work is cancelled and its outcome saved",
    ),
    ("/quit", "Alias for /exit"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub value: String,
    pub description: String,
}
pub struct Popup {
    pub items: Vec<Item>,
    pub selected: usize,
    pub offset: usize,
    pub area: Rect,
    pub range: Range<usize>,
    pub skill: bool,
}
pub fn skills(catalog: &Catalog) -> Vec<Item> {
    catalog
        .skills
        .iter()
        .filter(|(name, _)| !catalog.errors.contains_key(*name))
        .map(|(name, skill)| Item {
            value: format!("${name}"),
            description: skill.description.clone(),
        })
        .collect()
}

pub fn suggest(editor: &Editor, skills: &[Item]) -> Option<Popup> {
    let before = editor.text.get(..editor.cursor)?;
    let start = before
        .rfind(char::is_whitespace)
        .map_or(0, |i| i + before[i..].chars().next().unwrap().len_utf8());
    let token = &before[start..];
    let end = editor.text[editor.cursor..]
        .find(char::is_whitespace)
        .map_or(editor.text.len(), |i| editor.cursor + i);
    let (range, query, options, skill) = if token.starts_with('$')
        && before[..start]
            .split_whitespace()
            .all(|s| s.starts_with('$'))
    {
        (start..end, token.to_string(), skills.to_vec(), true)
    } else if before.starts_with('/') && !before.contains('\n') {
        let options = if before.starts_with("/stage ") {
            crate::runtime::STAGES
                .iter()
                .map(|(name, _, _)| Item {
                    value: format!("/stage {name}"),
                    description: "Workflow stage (new sessions only)".into(),
                })
                .collect()
        } else if before.starts_with("/language ") {
            [
                ("auto", "Follow user preference"),
                ("en", "English"),
                ("zh-TW", "Traditional Chinese (Taiwan)"),
                ("zh-CN", "Simplified Chinese"),
                ("ja-JP", "Japanese"),
                ("ko-KR", "Korean"),
            ]
            .into_iter()
            .map(|(tag, label)| Item {
                value: format!("/language {tag}"),
                description: label.into(),
            })
            .collect()
        } else if before.starts_with("/pm ") {
            crate::tools::specs()
                .into_iter()
                .find(|s| s.name == "pm")
                .into_iter()
                .flat_map(|spec| {
                    spec.parameters["properties"]["command"]["enum"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                })
                .filter_map(|value| value.as_str().map(str::to_owned))
                .map(|name| Item {
                    value: format!("/pm {name}"),
                    description: "Native PM command; arguments may be required".into(),
                })
                .collect()
        } else if start > 0 && !before.starts_with("/skills ") && !before.starts_with("/probe ") {
            return None;
        } else {
            COMMANDS
                .iter()
                .map(|(value, description)| Item {
                    value: (*value).into(),
                    description: (*description).into(),
                })
                .collect()
        };
        (0..end, before.to_string(), options, false)
    } else {
        return None;
    };
    let prefix = query.to_lowercase();
    let items = options
        .into_iter()
        .filter(|i| i.value.to_lowercase().starts_with(&prefix))
        .collect();
    Some(Popup {
        items,
        selected: 0,
        offset: 0,
        area: Rect::default(),
        range,
        skill,
    })
}

impl Popup {
    pub fn move_by(&mut self, down: bool) {
        self.selected = if down {
            (self.selected + 1).min(self.items.len().saturating_sub(1))
        } else {
            self.selected.saturating_sub(1)
        };
    }
    pub fn insert(&self, editor: &mut Editor) {
        let Some(item) = self.items.get(self.selected) else {
            return;
        };
        let trailing_space = !editor.text[self.range.end..].starts_with(char::is_whitespace);
        let text = format!("{}{}", item.value, if trailing_space { " " } else { "" });
        editor.text.replace_range(self.range.clone(), &text);
        editor.cursor = self.range.start + text.len();
    }
}

/// Recall is local to the running UI; Down restores the unfinished draft exactly.
#[derive(Default)]
pub struct History {
    entries: Vec<String>,
    position: Option<usize>,
    draft: Option<(String, usize)>,
}
impl History {
    pub fn remember(&mut self, text: &str) {
        self.edited();
        if !text.trim().is_empty() && self.entries.last().is_none_or(|last| last != text) {
            self.entries.push(text.into());
            if self.entries.len() > 200 {
                self.entries.remove(0);
            }
        }
    }
    pub fn edited(&mut self) {
        self.position = None;
        self.draft = None;
    }
    pub fn recall(&mut self, editor: &mut Editor, down: bool) {
        if down {
            let Some(position) = self.position else {
                return;
            };
            if position + 1 >= self.entries.len() {
                if let Some((text, cursor)) = self.draft.take() {
                    editor.text = text;
                    editor.cursor = cursor;
                }
                self.position = None;
                return;
            }
            self.position = Some(position + 1);
        } else {
            if self.entries.is_empty() {
                return;
            }
            if self.position.is_none() {
                self.draft = Some((editor.text.clone(), editor.cursor));
            }
            self.position = Some(
                self.position
                    .map_or(self.entries.len() - 1, |n| n.saturating_sub(1)),
            );
        }
        editor.text = self.entries[self.position.unwrap()].clone();
        editor.cursor = editor.text.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completes_cursor_token_preserves_suffix_and_only_activating_skill_prefixes() {
        let skills = vec![Item {
            value: "$risk-review".into(),
            description: "Review risks".into(),
        }];
        let mut e = Editor::default();
        e.insert("$ri analyse 中文");
        e.cursor = 3;
        suggest(&e, &skills).unwrap().insert(&mut e);
        assert_eq!(e.text, "$risk-review analyse 中文");
        e.clear();
        e.insert("Source says $ri");
        assert!(suggest(&e, &skills).is_none());
        e.clear();
        e.insert("$pm-review $ri");
        assert_eq!(suggest(&e, &skills).unwrap().range.start, 11);
        e.clear();
        e.insert("/sta prd");
        e.cursor = 4;
        let mut p = suggest(&e, &skills).unwrap();
        p.selected = p.items.iter().position(|i| i.value == "/stage").unwrap();
        p.insert(&mut e);
        assert_eq!(e.text, "/stage prd");
    }
    #[test]
    fn recall_restores_multiline_unicode_draft_and_cursor() {
        let mut h = History::default();
        h.remember("first");
        h.remember("second");
        let mut e = Editor::default();
        e.insert("未送出\ndraft");
        e.cursor = 3;
        h.recall(&mut e, false);
        assert_eq!(e.text, "second");
        h.recall(&mut e, false);
        assert_eq!(e.text, "first");
        h.recall(&mut e, true);
        h.recall(&mut e, true);
        assert_eq!(e.text, "未送出\ndraft");
        assert_eq!(e.cursor, 3);
    }
}
