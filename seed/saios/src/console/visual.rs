use efi_main::graphics::FramebufferInfo;

use super::FramebufferBenchResult;
use super::backend::ConsoleBackend;
use super::framebuffer::{DisplayProperties, FramebufferConsole};
use super::vga::VgaConsole;

#[derive(Copy, Clone, Eq, PartialEq)]
enum ActiveVisual {
    Vga,
    Framebuffer,
}

pub struct VisualConsole {
    vga: VgaConsole,
    framebuffer: FramebufferConsole,
    active: ActiveVisual,
}

impl VisualConsole {
    pub const fn new() -> Self {
        Self {
            vga: VgaConsole::new(),
            framebuffer: FramebufferConsole::new(),
            active: ActiveVisual::Vga,
        }
    }

    fn active_mut(&mut self) -> &mut dyn ConsoleBackend {
        match self.active {
            ActiveVisual::Vga => &mut self.vga,
            ActiveVisual::Framebuffer => &mut self.framebuffer,
        }
    }

    pub fn text_columns(&self) -> Option<usize> {
        match self.active {
            ActiveVisual::Vga => self.vga.text_columns(),
            ActiveVisual::Framebuffer => self.framebuffer.text_columns(),
        }
    }

    pub fn text_rows(&self) -> Option<usize> {
        match self.active {
            ActiveVisual::Vga => self.vga.text_rows(),
            ActiveVisual::Framebuffer => self.framebuffer.text_rows(),
        }
    }

    pub fn scrollback_lines(&self) -> usize {
        match self.active {
            ActiveVisual::Vga => self.vga.scrollback_lines(),
            ActiveVisual::Framebuffer => self.framebuffer.scrollback_lines(),
        }
    }

    pub fn view_offset(&self) -> usize {
        match self.active {
            ActiveVisual::Vga => self.vga.view_offset(),
            ActiveVisual::Framebuffer => self.framebuffer.view_offset(),
        }
    }

    #[allow(dead_code)]
    pub fn ensure_renderer_ready(&mut self) -> bool {
        match self.active {
            ActiveVisual::Vga => self.vga.ensure_renderer_ready(),
            ActiveVisual::Framebuffer => self.framebuffer.ensure_renderer_ready(),
        }
    }

    pub fn promote_framebuffer_renderer(&mut self) -> bool {
        if self.framebuffer.ensure_renderer_ready() {
            self.active = ActiveVisual::Framebuffer;
            return true;
        }

        false
    }

    pub fn framebuffer_attached(&self) -> bool {
        self.framebuffer.display_properties().is_some()
    }

    pub fn attach(&mut self, info: FramebufferInfo) {
        self.framebuffer.attach(info);
        if self.framebuffer.ensure_renderer_ready() {
            self.active = ActiveVisual::Framebuffer;
        } else {
            self.vga.attach(info);
            self.active = ActiveVisual::Vga;
        }
    }

    pub fn attach_direct(&mut self, info: FramebufferInfo) {
        self.framebuffer.attach_direct(info);
        if self.framebuffer.ensure_renderer_ready() {
            self.active = ActiveVisual::Framebuffer;
        } else {
            self.vga.attach_direct(info);
            self.active = ActiveVisual::Vga;
        }
    }

    pub fn display_properties(&self) -> Option<DisplayProperties> {
        match self.active {
            ActiveVisual::Vga => self.vga.display_properties(),
            ActiveVisual::Framebuffer => self.framebuffer.display_properties(),
        }
    }

    pub fn scroll_view_lines(&mut self, lines: isize) -> bool {
        match self.active {
            ActiveVisual::Vga => self.vga.scroll_view_lines(lines),
            ActiveVisual::Framebuffer => self.framebuffer.scroll_view_lines(lines),
        }
    }

    pub fn scroll_to_bottom(&mut self) -> bool {
        match self.active {
            ActiveVisual::Vga => self.vga.scroll_to_bottom(),
            ActiveVisual::Framebuffer => self.framebuffer.scroll_to_bottom(),
        }
    }

    pub fn benchmark_clears(&mut self, passes: usize) -> Option<FramebufferBenchResult> {
        match self.active {
            ActiveVisual::Vga => self.vga.benchmark_clears(passes),
            ActiveVisual::Framebuffer => self.framebuffer.benchmark_clears(passes),
        }
    }
}

impl ConsoleBackend for VisualConsole {
    fn put_char(&mut self, c: char) {
        self.active_mut().put_char(c);
    }

    fn put_str(&mut self, s: &str) {
        self.active_mut().put_str(s);
    }

    fn clear(&mut self) {
        self.active_mut().clear();
    }

    fn set_cursor(&mut self, x: usize, y: usize) {
        self.active_mut().set_cursor(x, y);
    }

    fn scroll_up(&mut self, rows: usize) -> bool {
        self.active_mut().scroll_up(rows)
    }

    fn blink_cursor(&mut self) {
        self.active_mut().blink_cursor();
    }
}
