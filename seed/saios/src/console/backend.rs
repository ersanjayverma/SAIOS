pub trait ConsoleBackend {
    fn put_char(&mut self, c: char);
    fn clear(&mut self);
    fn set_cursor(&mut self, x: usize, y: usize);
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

    fn clear(&mut self) {
        self.left.clear();
        self.right.clear();
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.left.set_cursor(x, y);
        self.right.set_cursor(x, y);
    }
}
