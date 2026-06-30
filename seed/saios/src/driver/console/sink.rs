pub trait ConsoleSink {
    fn put_char(&mut self, ch: char);

    fn write_str(&mut self, s: &str) {
        for ch in s.chars() {
            self.put_char(ch);
        }
    }

    fn clear(&mut self) {}
}
