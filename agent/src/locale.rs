use crate::workspace::{MAX_FILE, Workspace};
use anyhow::{Context, Result, bail, ensure};
use std::collections::BTreeMap;

#[derive(Debug)]
enum Part {
    Text(String),
    Field(String),
}
fn parse(text: &str) -> Result<Vec<Part>> {
    let mut chars = text.chars().peekable();
    let mut parts = vec![];
    let mut literal = String::new();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '{' => {
                parts.push(Part::Text(std::mem::take(&mut literal)));
                let mut field = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    ensure!(c != '{', "Nested locale placeholder");
                    field.push(c);
                }
                ensure!(
                    closed && !field.is_empty(),
                    "Unclosed or empty locale placeholder"
                );
                parts.push(Part::Field(field));
            }
            '}' => bail!("Unmatched locale brace"),
            _ => literal.push(c),
        }
    }
    parts.push(Part::Text(literal));
    Ok(parts)
}
fn fields(parts: &[Part]) -> Vec<&str> {
    let mut fields: Vec<_> = parts
        .iter()
        .filter_map(|p| match p {
            Part::Field(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    fields.sort();
    fields
}
pub(crate) fn language_tag(raw: &str) -> Result<String> {
    let mut parts = raw.split('-');
    let first = parts.next().unwrap_or("");
    ensure!(
        (2..=8).contains(&first.len()) && first.bytes().all(|c| c.is_ascii_alphabetic()),
        "document_language must be a language tag such as zh-TW, en, or ja-JP"
    );
    let mut tag = first.to_ascii_lowercase();
    for part in parts {
        ensure!(
            (1..=8).contains(&part.len()) && part.bytes().all(|c| c.is_ascii_alphanumeric()),
            "Invalid document_language subtag"
        );
        tag.push('-');
        if part.len() == 2 && part.bytes().all(|c| c.is_ascii_alphabetic()) {
            tag.push_str(&part.to_ascii_uppercase());
        } else if part.len() == 4 && part.bytes().all(|c| c.is_ascii_alphabetic()) {
            tag.push_str(&part[..1].to_ascii_uppercase());
            tag.push_str(&part[1..].to_ascii_lowercase());
        } else {
            tag.push_str(&part.to_ascii_lowercase());
        }
    }
    Ok(tag)
}
pub(crate) struct Catalog {
    pub language: String,
    templates: BTreeMap<String, Vec<Part>>,
}
impl Catalog {
    pub fn load(workspace: &Workspace) -> Result<Self> {
        let language = language_tag(&workspace.language()?)?;
        let renderer = if language.split('-').next() == Some("en") {
            "en"
        } else {
            &language
        };
        let path = workspace.path(&format!("tools/locales/{renderer}.json"))?;
        ensure!(
            path.is_file(),
            "No project-document renderer for {language}. Translate and review tools/locales/{language}.json before generating reports."
        );
        ensure!(
            std::fs::metadata(&path)?.len() <= MAX_FILE,
            "Language catalog exceeds text limit"
        );
        let base: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../../tools/locales/en.json"))?;
        let data: BTreeMap<String, String> =
            serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        ensure!(
            base.keys().eq(data.keys()) && data.values().all(|s| !s.trim().is_empty()),
            "Language catalog must contain exactly the English catalog keys with nonempty strings"
        );
        let mut templates = BTreeMap::new();
        for (key, value) in data {
            let parsed =
                parse(&value).with_context(|| format!("Invalid locale template: {key}"))?;
            ensure!(
                fields(&parsed) == fields(&parse(&base[&key])?),
                "Language catalog changed formatting placeholders: {key}"
            );
            templates.insert(key, parsed);
        }
        Ok(Self {
            language,
            templates,
        })
    }
    pub fn text(&self, key: &str) -> Result<String> {
        self.render(key, &[])
    }
    pub fn render(&self, key: &str, values: &[(&str, String)]) -> Result<String> {
        let mut text = String::new();
        for part in self
            .templates
            .get(key)
            .with_context(|| format!("Missing locale key: {key}"))?
        {
            match part {
                Part::Text(s) => text.push_str(s),
                Part::Field(name) => text.push_str(
                    &values
                        .iter()
                        .find(|(k, _)| *k == name)
                        .with_context(|| format!("Missing locale value: {name}"))?
                        .1,
                ),
            }
        }
        Ok(text)
    }
}
