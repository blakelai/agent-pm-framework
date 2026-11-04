use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default, Clone)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    pub links: Vec<String>,
}
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .replace('\t', "    ")
}
pub fn wrap(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out = vec![];
    for line in lines {
        let mut row = Line::default();
        let mut col = 0;
        for span in line.spans {
            for word in span.content.split_word_bounds() {
                if !word.chars().any(char::is_whitespace)
                    && word.width() <= width
                    && col > 0
                    && col + word.width() > width
                {
                    out.push(row);
                    row = Line::default();
                    col = 0;
                }
                for g in word.graphemes(true) {
                    if g == "\n" {
                        out.push(row);
                        row = Line::default();
                        col = 0;
                        continue;
                    }
                    let cells = g.width();
                    if col + cells > width && col > 0 {
                        out.push(row);
                        row = Line::default();
                        col = 0;
                        if g == " " {
                            continue;
                        }
                    }
                    row.spans.push(Span::styled(g.to_owned(), span.style));
                    col += cells;
                }
            }
        }
        out.push(row);
    }
    if out.is_empty() {
        out.push(Line::default());
    }
    out
}
pub fn render(source: &str, width: usize) -> Rendered {
    let code_ranges = Parser::new(source)
        .into_offset_iter()
        .filter_map(|(e, range)| {
            matches!(e, Event::Code(_) | Event::Start(Tag::CodeBlock(_))).then_some(range)
        })
        .collect::<Vec<_>>();
    let wiki = regex::Regex::new(r"\[\[([^\]\n]+)\]\]").unwrap();
    let source = wiki.replace_all(source, |cap: &regex::Captures| {
        if code_ranges
            .iter()
            .any(|r| r.contains(&cap.get(0).unwrap().start()))
        {
            return cap[0].to_string();
        }
        let (target, label) = cap[1].split_once('|').unwrap_or((&cap[1], &cap[1]));
        format!("[{label}](<wiki:{target}>)")
    });
    let mut lines = vec![];
    let mut current = Line::default();
    let mut style = Style::default();
    let mut stack = vec![];
    let mut list: Vec<Option<u64>> = vec![];
    let mut links = vec![];
    let mut link_stack = vec![];
    let mut quote = 0;
    let mut table = false;
    let mut cell = String::new();
    let mut row = vec![];
    let mut rows: Vec<Vec<String>> = vec![];
    let flush = |current: &mut Line<'static>, lines: &mut Vec<Line<'static>>| {
        if !current.spans.is_empty() {
            lines.push(std::mem::take(current));
        }
    };
    for event in Parser::new_ext(
        &source,
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH,
    ) {
        match event {
            Event::Start(tag) => {
                stack.push(style);
                match tag {
                    Tag::Heading { .. } => {
                        flush(&mut current, &mut lines);
                        style = style.fg(Color::Cyan).add_modifier(Modifier::BOLD);
                    }
                    Tag::Strong => style = style.add_modifier(Modifier::BOLD),
                    Tag::Emphasis => style = style.add_modifier(Modifier::ITALIC),
                    Tag::Strikethrough => style = style.add_modifier(Modifier::CROSSED_OUT),
                    Tag::BlockQuote(_) => {
                        flush(&mut current, &mut lines);
                        quote += 1;
                        style = style.fg(Color::Gray);
                    }
                    Tag::List(start) => {
                        flush(&mut current, &mut lines);
                        list.push(start);
                    }
                    Tag::Item => {
                        flush(&mut current, &mut lines);
                        let marker = match list.last_mut() {
                            Some(Some(n)) => {
                                let s = format!("{n}. ");
                                *n += 1;
                                s
                            }
                            _ => "• ".into(),
                        };
                        current.spans.push(Span::raw(format!(
                            "{}{marker}",
                            "  ".repeat(list.len().saturating_sub(1))
                        )));
                    }
                    Tag::CodeBlock(kind) => {
                        flush(&mut current, &mut lines);
                        let language = match kind {
                            pulldown_cmark::CodeBlockKind::Fenced(s) => s.to_string(),
                            _ => String::new(),
                        };
                        if !language.is_empty() {
                            lines.push(Line::styled(
                                format!("  {language}"),
                                Style::default().fg(Color::DarkGray),
                            ));
                        }
                        style = style.fg(Color::Yellow);
                    }
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                        links.push(dest_url.to_string());
                        link_stack.push(links.len());
                        style = style.fg(Color::Blue).add_modifier(Modifier::UNDERLINED);
                    }
                    Tag::Table(_) => {
                        flush(&mut current, &mut lines);
                        table = true;
                        rows.clear();
                    }
                    Tag::TableHead | Tag::TableRow => row.clear(),
                    Tag::TableCell => cell.clear(),
                    Tag::Paragraph if quote > 0 => {
                        current.spans.push(Span::styled("│ ".repeat(quote), style))
                    }
                    _ => {}
                }
            }
            Event::End(tag) => {
                match tag {
                    TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock => {
                        flush(&mut current, &mut lines);
                        lines.push(Line::default());
                    }
                    TagEnd::Item => flush(&mut current, &mut lines),
                    TagEnd::List(_) => {
                        list.pop();
                    }
                    TagEnd::BlockQuote(_) => {
                        quote = quote.saturating_sub(1);
                    }
                    TagEnd::Link | TagEnd::Image => {
                        if let Some(n) = link_stack.pop() {
                            if table {
                                cell.push_str(&format!(" [{n}]"));
                            } else {
                                current.spans.push(Span::styled(format!(" [{n}]"), style));
                            }
                        }
                    }
                    TagEnd::TableCell => row.push(std::mem::take(&mut cell)),
                    TagEnd::TableHead | TagEnd::TableRow => rows.push(std::mem::take(&mut row)),
                    TagEnd::Table => {
                        table = false;
                        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
                        let widths = (0..columns)
                            .map(|i| {
                                rows.iter()
                                    .filter_map(|r| r.get(i))
                                    .map(|s| s.width())
                                    .max()
                                    .unwrap_or(0)
                                    .min(40)
                            })
                            .collect::<Vec<_>>();
                        for (n, row) in rows.iter().enumerate() {
                            let cells = row
                                .iter()
                                .enumerate()
                                .map(|(i, s)| {
                                    format!(
                                        "{}{}",
                                        s,
                                        " ".repeat(widths[i].saturating_sub(s.width()))
                                    )
                                })
                                .collect::<Vec<_>>();
                            lines.push(Line::styled(
                                cells.join(" │ "),
                                if n == 0 {
                                    Style::default().add_modifier(Modifier::BOLD)
                                } else {
                                    Style::default()
                                },
                            ));
                            if n == 0 {
                                lines.push(Line::raw(
                                    widths
                                        .iter()
                                        .map(|w| "─".repeat(*w))
                                        .collect::<Vec<_>>()
                                        .join("─┼─"),
                                ));
                            }
                        }
                        lines.push(Line::default());
                    }
                    _ => {}
                }
                style = stack.pop().unwrap_or_default();
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                let text = clean(&text);
                if table {
                    cell.push_str(&text);
                } else {
                    current.spans.push(Span::styled(text, style));
                }
            }
            Event::Code(text) => {
                if table {
                    cell.push_str(&clean(&text));
                } else {
                    current
                        .spans
                        .push(Span::styled(clean(&text), style.fg(Color::Yellow)));
                }
            }
            Event::SoftBreak => {
                if table {
                    cell.push(' ');
                } else {
                    current.spans.push(Span::raw(" "));
                }
            }
            Event::HardBreak => flush(&mut current, &mut lines),
            Event::TaskListMarker(checked) => {
                current
                    .spans
                    .push(Span::raw(if checked { "☑ " } else { "☐ " }))
            }
            Event::Rule => {
                flush(&mut current, &mut lines);
                lines.push(Line::styled(
                    "─".repeat(width.min(60)),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            Event::FootnoteReference(text) => current.spans.push(Span::raw(format!("[{text}]"))),
            _ => {}
        }
    }
    flush(&mut current, &mut lines);
    Rendered {
        lines: wrap(lines, width),
        links,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readable_words_wrap_without_splitting() {
        let r = render("Review technical questions.", 16);
        let lines = r
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert!(lines.iter().any(|l| l.contains("technical")));
        assert!(lines.iter().any(|l| l.contains("questions")));
        assert!(lines.iter().all(|l| l.width() <= 16));
    }

    #[test]
    fn wiki_syntax_in_code_is_preserved() {
        let r = render(
            "\u{60}[[literal]]\u{60}\n\n\u{60}\u{60}\u{60}\n[[code]]\n\u{60}\u{60}\u{60}",
            80,
        );
        let text = r
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<String>();
        assert!(text.contains("[[literal]]"));
        assert!(text.contains("[[code]]"));
        assert!(r.links.is_empty());
    }

    #[test]
    fn renders_markdown_and_wiki_links_with_safe_cells() {
        let r = render(
            "# Title\n\n**Bold** and [source](file.md) [[02_Domain/CONTEXT|Domain]]\n\n- [x] Done\n\n| A | B |\n|---|---|\n| 中 | value |\n\n\u{1b}[31m",
            25,
        );
        let text = r
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("**"));
        assert!(!text.contains("# Title"));
        assert!(text.contains("☑"));
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("│"));
        assert_eq!(r.links, ["file.md", "wiki:02_Domain/CONTEXT"]);
        assert!(r.lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::BOLD))
        }));
        assert!(r.lines.iter().all(|l| l.width() <= 25));
    }
    #[test]
    fn incomplete_stream_and_wide_graphemes_remain_renderable() {
        for s in [
            "**partial",
            "\u{60}\u{60}\u{60}rust\nlet x",
            "中文👩‍💻e\u{301}",
        ] {
            assert!(!render(s, 4).lines.is_empty());
        }
    }
}
