use heapless::{String, Vec};

pub struct InputBuffer {
    line: Vec<char, 256>,
    cursor: usize,
    history: Vec<String<256>, 64>,
    history_index: Option<usize>,
    stashed_current: String<256>,
}

impl InputBuffer {
    pub const fn new() -> Self {
        Self {
            line: Vec::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            stashed_current: String::new(),
        }
    }

    pub fn insert(&mut self, ch: char) -> bool {
        if self.line.insert(self.cursor, ch).is_err() {
            return false;
        }
        self.cursor = self.cursor.saturating_add(1);
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        let idx = self.cursor - 1;
        let _ = self.line.remove(idx);

        self.cursor = idx;
        true
    }

    pub fn delete(&mut self) -> bool {
        if self.cursor >= self.line.len() {
            return false;
        }

        let _ = self.line.remove(self.cursor);
        true
    }

    pub fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        self.cursor -= 1;
        true
    }

    pub fn move_right(&mut self) -> bool {
        if self.cursor >= self.line.len() {
            return false;
        }

        self.cursor += 1;
        true
    }

    pub fn move_home(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        self.cursor = 0;
        true
    }

    pub fn move_end(&mut self) -> bool {
        if self.cursor == self.line.len() {
            return false;
        }

        self.cursor = self.line.len();
        true
    }

    pub fn move_prev_word(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        let mut pos = self.cursor;
        while pos > 0 && self.line[pos - 1].is_whitespace() {
            pos -= 1;
        }
        while pos > 0 && !self.line[pos - 1].is_whitespace() {
            pos -= 1;
        }

        if pos == self.cursor {
            return false;
        }

        self.cursor = pos;
        true
    }

    pub fn move_next_word(&mut self) -> bool {
        if self.cursor >= self.line.len() {
            return false;
        }

        let mut pos = self.cursor;
        while pos < self.line.len() && !self.line[pos].is_whitespace() {
            pos += 1;
        }
        while pos < self.line.len() && self.line[pos].is_whitespace() {
            pos += 1;
        }

        if pos == self.cursor {
            return false;
        }

        self.cursor = pos;
        true
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn char_left_of_cursor(&self) -> Option<char> {
        if self.cursor == 0 {
            return None;
        }
        self.line.get(self.cursor - 1).copied()
    }

    pub fn char_at_cursor(&self) -> Option<char> {
        self.line.get(self.cursor).copied()
    }

    pub fn render(&self) -> String<256> {
        let mut out = String::new();
        for ch in &self.line {
            let _ = out.push(*ch);
        }
        out
    }

    pub fn set_line(&mut self, line: &str) {
        self.line.clear();
        for ch in line.chars() {
            if self.line.push(ch).is_err() {
                break;
            }
        }
        self.cursor = self.line.len();
    }

    pub fn clear(&mut self) {
        self.line.clear();
        self.cursor = 0;
        self.history_index = None;
        self.stashed_current.clear();
    }

    pub fn clear_to_start(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        self.line.drain(0..self.cursor);
        self.cursor = 0;
        true
    }

    pub fn clear_to_end(&mut self) -> bool {
        if self.cursor >= self.line.len() {
            return false;
        }

        let end = self.line.len();
        self.line.drain(self.cursor..end);
        true
    }

    pub fn delete_prev_word(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }

        let mut start = self.cursor;
        while start > 0 && self.line[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !self.line[start - 1].is_whitespace() {
            start -= 1;
        }

        self.line.drain(start..self.cursor);
        self.cursor = start;
        true
    }

    pub fn history_prev(&mut self) -> Option<String<256>> {
        if self.history.is_empty() {
            return None;
        }

        if self.history_index.is_none() {
            self.stashed_current = self.render();
            self.history_index = Some(self.history.len().saturating_sub(1));
        } else if let Some(index) = self.history_index {
            self.history_index = Some(index.saturating_sub(1));
        }

        let index = self.history_index?;
        self.history.get(index).cloned()
    }

    pub fn history_next(&mut self) -> Option<String<256>> {
        let index = self.history_index?;

        if index + 1 < self.history.len() {
            self.history_index = Some(index + 1);
            return self.history.get(index + 1).cloned();
        }

        self.history_index = None;
        Some(self.stashed_current.clone())
    }

    pub fn submit(&mut self) -> String<256> {
        let line = self.render();

        if !line.is_empty() {
            if self.history.len() == self.history.capacity() {
                let _ = self.history.remove(0);
            }
            let _ = self.history.push(line.clone());
        }

        self.history_index = None;
        self.stashed_current.clear();

        self.line.clear();
        self.cursor = 0;

        line
    }

}
