pub trait ConsoleBackend {
    fn put_char(&mut self, c: char);

    /// Writes a whole string to the backend.
    fn put_str(&mut self, s: &str) {
        for c in s.chars() {
            self.put_char(c);
        }
    }

    fn clear(&mut self);
    fn set_cursor(&mut self, x: usize, y: usize);

    fn scroll_up(&mut self, _rows: usize) -> bool {
        false
    }

    /// Advance the visible cursor blink state.  Called from the timer tick
    /// path.  The default implementation is a no-op for backends that do not
    /// implement a visible cursor.
    fn blink_cursor(&mut self) {}
}

pub struct MirrorConsole<A, B> {
    left: A,
    right: B,
}

impl<A, B> MirrorConsole<A, B> {
    pub const fn new(left: A, right: B) -> Self {
        Self { left, right }
    }

    pub fn right_mut(&mut self) -> &mut B {
        &mut self.right
    }
}

impl<A, B> ConsoleBackend for MirrorConsole<A, B>
where
    A: ConsoleBackend,
    B: ConsoleBackend,
{
    fn put_char(&mut self, c: char) {
        self.left.put_char(c);
        self.right.put_char(c);
    }

    /// Mirrors a full string to both backends with a single call site.
    fn put_str(&mut self, s: &str) {
        self.left.put_str(s);
        self.right.put_str(s);
    }

    fn clear(&mut self) {
        self.left.clear();
        self.right.clear();
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.left.set_cursor(x, y);
        self.right.set_cursor(x, y);
    }

    fn scroll_up(&mut self, rows: usize) -> bool {
        let left_ok = self.left.scroll_up(rows);
        let right_ok = self.right.scroll_up(rows);
        left_ok && right_ok
    }

    fn blink_cursor(&mut self) {
        self.left.blink_cursor();
        self.right.blink_cursor();
    }
}
