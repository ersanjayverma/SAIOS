use heapless::String;

pub struct InputBuffer {
    buffer: String<256>,
}

impl InputBuffer {
    pub const fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    pub fn push(&mut self, ch: char) {
        let _ = self.buffer.push(ch);
    }

    pub fn backspace(&mut self) -> bool {
        self.buffer.pop().is_some()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        self.buffer.as_str()
    }

    pub fn take(&mut self) -> String<256> {
        let mut out = String::new();
        let _ = out.push_str(self.buffer.as_str());
        self.buffer.clear();
        out
    }
}
