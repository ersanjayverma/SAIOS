/// Console text-grid cursor state.
///
/// Tracks the logical cursor position plus a visible blink phase.  The blink
/// is driven by the timer interrupt; higher layers call [`Cursor::blink_on`]
/// every timer tick (or at a multiple of it) to toggle visibility.
pub struct Cursor {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    blink_phase: bool,
    blink_divisor: u8,
    blink_counter: u8,
}

impl Cursor {
    /// Blink interval in timer ticks.  At 100 Hz this gives a 500 ms period.
    pub const DEFAULT_BLINK_DIVISOR: u8 = 50;

    pub const fn new(width: usize, height: usize) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
            blink_phase: false,
            blink_divisor: Self::DEFAULT_BLINK_DIVISOR,
            blink_counter: 0,
        }
    }

    /// Advance the blink counter and toggle phase when it reaches the divisor.
    /// Returns `true` if the visible blink state changed.
    pub fn blink_on(&mut self) -> bool {
        self.blink_counter = self.blink_counter.saturating_add(1);
        if self.blink_counter >= self.blink_divisor {
            self.blink_counter = 0;
            self.blink_phase = !self.blink_phase;
            return true;
        }
        false
    }

    /// Force the cursor visible (e.g., after movement or key input).
    pub fn show(&mut self) {
        self.blink_phase = true;
        self.blink_counter = 0;
    }
}
