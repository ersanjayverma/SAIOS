#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Deadline {
    pub expires_at: u64,
}

impl Deadline {
    pub const fn new(expires_at: u64) -> Self {
        Self { expires_at }
    }

    pub fn is_expired(&self, now_ns: u64) -> bool {
        now_ns >= self.expires_at
    }
}
