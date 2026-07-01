pub struct Cursor {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Cursor {
    pub const fn new(width: usize, height: usize) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}
