use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default, Debug)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}
impl Editor {
    pub fn insert(&mut self, value: &str) {
        let value = value
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ");
        let value: String = value
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect();
        self.text.insert_str(self.cursor, &value);
        self.cursor += value.len();
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
    pub fn click(&mut self, width: usize, row: usize, column: usize) {
        let width = width.max(1);
        let mut y = 0;
        let mut x = 0;
        for (offset, g) in self.text.grapheme_indices(true) {
            if g != "\n" && x + g.width() > width {
                y += 1;
                x = 0;
            }
            if y > row || y == row && (x >= column || g == "\n") {
                self.cursor = offset;
                return;
            }
            if g == "\n" {
                y += 1;
                x = 0;
            } else {
                x += g.width();
            }
        }
        self.cursor = self.text.len();
    }
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }
    fn left(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }
    fn right(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map(|g| self.cursor + g.len())
            .unwrap_or(self.cursor)
    }
    fn home(&self) -> usize {
        self.text[..self.cursor]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0)
    }
    fn end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map(|p| self.cursor + p)
            .unwrap_or(self.text.len())
    }
    fn vertical(&mut self, down: bool) {
        let start = self.home();
        let col = self.text[start..self.cursor].graphemes(true).count();
        let target = if down {
            let end = self.end();
            if end == self.text.len() {
                return;
            }
            end + 1
        } else {
            if start == 0 {
                return;
            }
            self.text[..start - 1]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0)
        };
        let line = self.text[target..].split('\n').next().unwrap_or("");
        self.cursor = target + line.graphemes(true).take(col).map(str::len).sum::<usize>();
    }
    pub fn key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert("\n")
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&c.to_string())
            }
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Left => self.cursor = self.left(),
            KeyCode::Right => self.cursor = self.right(),
            KeyCode::Up => self.vertical(false),
            KeyCode::Down => self.vertical(true),
            KeyCode::Home => self.cursor = self.home(),
            KeyCode::End => self.cursor = self.end(),
            KeyCode::Backspace => {
                let start = self.left();
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
            }
            KeyCode::Delete => {
                self.text.replace_range(self.cursor..self.right(), "");
            }
            KeyCode::Tab => self.insert("    "),
            _ => {}
        }
    }
    /// Display rows and caret use the same grapheme/cell layout.
    pub fn rows(&self, width: usize) -> (Vec<String>, (usize, usize)) {
        let width = width.max(1);
        let mut lines = vec![String::new()];
        let mut col = 0;
        let mut caret = (0, 0);
        for (offset, g) in self.text.grapheme_indices(true) {
            let cells = g.width();
            if g != "\n" && col + cells > width {
                lines.push(String::new());
                col = 0;
            }
            if offset == self.cursor {
                caret = (lines.len() - 1, col);
            }
            if g == "\n" {
                lines.push(String::new());
                col = 0;
            } else {
                lines.last_mut().unwrap().push_str(g);
                col += cells;
            }
        }
        if self.cursor == self.text.len() {
            if col >= width {
                lines.push(String::new());
                col = 0;
            }
            caret = (lines.len() - 1, col);
        }
        (lines, caret)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_graphemes_and_multiline_paste_without_submission() {
        let mut e = Editor::default();
        e.insert("中文👩‍💻\r\nsecond");
        e.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        e.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        e.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(e.text, "中文second");
        e.insert("\n");
        e.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        e.insert("A");
        assert_eq!(e.text, "A中文\nsecond");
    }
    #[test]
    fn caret_wraps_by_terminal_cells() {
        let mut e = Editor::default();
        e.insert("中a");
        assert_eq!(e.rows(3).1, (1, 0));
        e.insert("e\u{301}");
        assert_eq!(e.rows(3).1, (1, 1));
    }
}
