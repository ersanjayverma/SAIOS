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

    pub fn len(&self) -> usize {
        self.line.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
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
