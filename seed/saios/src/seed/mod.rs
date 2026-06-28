use efi_main::SaiosBootInfo;
use efi_main::graphics::FramebufferInfo;
use efi_main::graphics::PixelFormat;

use crate::arch;
use crate::diagnostics;
use crate::graphics::backbuffer::BackBuffer;
use crate::graphics::contracts::Renderer;
use crate::graphics::fonts::bitmap::BitmapFont;
use crate::graphics::software::renderer::SoftwareRenderer;
use crate::graphics::{Color, Point};
use crate::rrod;
use crate::timer::TimerManager;
const SAIOS_BLUE: Color = Color::rgb(14, 60, 128);

#[unsafe(no_mangle)]
extern "C" fn seed_exception_from_stack(stack: *const u64, vector: u32, has_error_code: u32) -> ! {
    diagnostics::exception_trap(vector);
    let context = rrod::capture::from_exception_stack(stack, vector, has_error_code != 0);
    rrod::trigger(context)
}

pub fn init(boot_info: *const SaiosBootInfo) {
    diagnostics::init_serial();

    rrod::set_boot_info(boot_info);

    // Install trap handlers as early as possible so pre-dashboard faults still hit RRoD.
    arch::install_exception_handlers();

    let boot_info = unsafe { &*boot_info };
    let mut init = KernelInit::new(boot_info);
    init.stage0_cpu();
    init.stage1_exceptions();
    init.stage2_memory();
    init.stage3_drivers();
    init.stage4_logging();
    init.stage5_graphics();
    init.stage6_desktop();
    init.stage7_runtime();
}

pub fn run() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

struct KernelInit<'a> {
    boot_info: &'a SaiosBootInfo,
    fb: FramebufferInfo,
    backbuffer: Option<BackBuffer>,
    dashboard: BootDashboard,
    timer: Option<TimerManager>,
}

impl<'a> KernelInit<'a> {
    fn new(boot_info: &'a SaiosBootInfo) -> Self {
        let mut dashboard = BootDashboard::new();
        dashboard.begin();
        dashboard.mark_ok(STEP_SERIAL);

        Self {
            boot_info,
            fb: boot_info.framebuffer,
            backbuffer: None,
            dashboard,
            timer: None,
        }
    }

    fn stage0_cpu(&mut self) {
        clear_framebuffer(&self.fb, (14, 60, 128));

        self.backbuffer = Some(BackBuffer::new(
            self.fb.width as u32,
            self.fb.height as u32,
            self.fb.stride,
            self.fb.bpp as u8,
            self.fb.pixel_format == PixelFormat::Bgr,
        ));

        self.dashboard.mark_ok(STEP_CPU);

        if let Some(backbuffer) = self.backbuffer.as_mut() {
            let mut surface = backbuffer.surface();
            let mut renderer = SoftwareRenderer::from_surface(&mut surface);
            let font = BitmapFont::new_5x7();
            self.dashboard.render(&mut renderer, &font);
            backbuffer.blit_to_framebuffer(self.boot_info.framebuffer.base as *mut u8);
        }
    }

    fn stage1_exceptions(&mut self) {
        self.dashboard.mark_ok(STEP_EXCEPTIONS);
    }

    fn stage2_memory(&mut self) {
        self.dashboard.mark_ok(STEP_MEMORY);
        self.dashboard.mark_ok(STEP_HEAP);

        crate::timer::init();
        self.timer = Some(TimerManager::new());
        self.dashboard.mark_ok(STEP_CYCLE_CLOCK);
    }

    fn stage3_drivers(&mut self) {
        self.dashboard.mark_waiting(STEP_KEYBOARD);
    }

    fn stage4_logging(&mut self) {
        diagnostics::init_logger();
        self.dashboard.mark_ok(STEP_LOGGER);
    }

    fn stage5_graphics(&mut self) {
        self.dashboard.mark_ok(STEP_GRAPHICS);

        if self.backbuffer.is_none() {
            self.backbuffer = Some(BackBuffer::new(
                self.fb.width as u32,
                self.fb.height as u32,
                self.fb.stride,
                self.fb.bpp as u8,
                self.fb.pixel_format == PixelFormat::Bgr,
            ));
        }
        self.dashboard.mark_ok(STEP_BACKBUFFER);

        let backbuffer = self.backbuffer.as_mut().unwrap();
        let mut surface = backbuffer.surface();
        let mut renderer = SoftwareRenderer::from_surface(&mut surface);
        let font = BitmapFont::new_5x7();

        self.dashboard.render(&mut renderer, &font);
        self.dashboard.mark_waiting(STEP_BMP_DECODER);

        self.dashboard.render(&mut renderer, &font);
    }

    fn stage6_desktop(&mut self) {
        if let Some(backbuffer) = self.backbuffer.as_mut() {
            let mut surface = backbuffer.surface();
            let mut renderer = SoftwareRenderer::from_surface(&mut surface);
            let font = BitmapFont::new_5x7();

            self.dashboard.mark_ok(STEP_DESKTOP);
            self.dashboard.render(&mut renderer, &font);
        }
    }

    fn stage7_runtime(&mut self) {
        let (elapsed_ms, elapsed_ns) = if let Some(timer) = self.timer.as_ref() {
            (timer.monotonic_ms(), timer.monotonic_ns())
        } else {
            (0, 0)
        };

        self.dashboard.finish(elapsed_ms, elapsed_ns);

        if let Some(backbuffer) = self.backbuffer.as_mut() {
            let mut surface = backbuffer.surface();
            let mut renderer = SoftwareRenderer::from_surface(&mut surface);
            let font = BitmapFont::new_5x7();
            self.dashboard.render(&mut renderer, &font);
        }

        if let Some(backbuffer) = self.backbuffer.as_ref() {
            backbuffer.blit_to_framebuffer(self.boot_info.framebuffer.base as *mut u8);
        }
    }
}

const STEP_CPU: usize = 0;
const STEP_EXCEPTIONS: usize = 1;
const STEP_SERIAL: usize = 2;
const STEP_CYCLE_CLOCK: usize = 3;
const STEP_LOGGER: usize = 4;
const STEP_MEMORY: usize = 5;
const STEP_HEAP: usize = 6;
const STEP_GRAPHICS: usize = 7;
const STEP_BACKBUFFER: usize = 8;
const STEP_BMP_DECODER: usize = 9;
const STEP_DESKTOP: usize = 10;
const STEP_KEYBOARD: usize = 11;

#[derive(Copy, Clone, Eq, PartialEq)]
enum BootStepState {
    Pending,
    Ok,
    Waiting,
}

#[derive(Copy, Clone)]
struct BootStep {
    name: &'static str,
    state: BootStepState,
}

impl BootStep {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            state: BootStepState::Pending,
        }
    }
}

struct BootDashboard {
    steps: [BootStep; 12],
    finalized: bool,
}

impl BootDashboard {
    fn new() -> Self {
        Self {
            steps: [
                BootStep::new("CPU"),
                BootStep::new("Exceptions"),
                BootStep::new("Serial"),
                BootStep::new("Cycle Clock"),
                BootStep::new("Logger"),
                BootStep::new("Memory"),
                BootStep::new("Heap"),
                BootStep::new("Graphics"),
                BootStep::new("Backbuffer"),
                BootStep::new("BMP Decoder"),
                BootStep::new("Desktop"),
                BootStep::new("Keyboard"),
            ],
            finalized: false,
        }
    }

    fn begin(&mut self) {
        crate::drivers::serial::write_str("SAIOS DEVELOPMENT BUILD\n");
        crate::drivers::serial::write_str("Initializing...\n");
    }

    fn mark_ok(&mut self, index: usize) {
        self.set_state(index, BootStepState::Ok);
    }

    fn mark_waiting(&mut self, index: usize) {
        self.set_state(index, BootStepState::Waiting);
    }

    fn set_state(&mut self, index: usize, state: BootStepState) {
        if index >= self.steps.len() || self.steps[index].state == state {
            return;
        }

        self.steps[index].state = state;
        self.log_step(index);
    }

    fn log_step(&self, index: usize) {
        let step = &self.steps[index];
        crate::drivers::serial::write_str("[BOOT] ");
        crate::drivers::serial::write_str(step.name);

        let dots = 24usize.saturating_sub(step.name.len());
        for _ in 0..dots {
            crate::drivers::serial::write_str(".");
        }

        crate::drivers::serial::write_str(" ");
        crate::drivers::serial::write_str(match step.state {
            BootStepState::Pending => "PENDING",
            BootStepState::Ok => "OK",
            BootStepState::Waiting => "WAITING",
        });
        crate::drivers::serial::write_str("\n");
    }

    fn finish(&mut self, elapsed_ms: u64, elapsed_ns: u64) {
        if self.finalized {
            return;
        }

        self.mark_waiting(STEP_KEYBOARD);
        self.finalized = true;

        crate::drivers::serial::write_fmt(format_args!(
            "Boot completed in {} ms ({} ns)\n",
            elapsed_ms, elapsed_ns
        ));
        crate::drivers::serial::write_str("Waiting for input...\n");
    }

    fn render<T: Renderer>(&self, renderer: &mut T, font: &BitmapFont) {
        renderer.clear(SAIOS_BLUE);
        renderer.draw_text(
            Point { x: 10, y: 10 },
            "SAIOS DEVELOPMENT BUILD",
            font,
            Color::rgb(240, 245, 255),
        );
        renderer.draw_text(
            Point { x: 10, y: 20 },
            "Initializing...",
            font,
            Color::rgb(180, 200, 240),
        );

        let mut y = 34;
        for step in self.steps.iter() {
            let (status, color) = match step.state {
                BootStepState::Pending => ("[ ... ]", Color::rgb(160, 170, 190)),
                BootStepState::Ok => ("[ OK  ]", Color::rgb(110, 230, 140)),
                BootStepState::Waiting => ("[WAIT ]", Color::rgb(255, 210, 120)),
            };

            renderer.draw_text(
                Point { x: 10, y },
                step.name,
                font,
                Color::rgb(240, 245, 255),
            );
            renderer.draw_text(Point { x: 140, y }, status, font, color);
            y += 9;
        }

        let footer = if self.finalized {
            "Waiting for input..."
        } else {
            "Booting subsystems..."
        };
        renderer.draw_text(
            Point { x: 10, y: y + 6 },
            footer,
            font,
            Color::rgb(190, 210, 245),
        );
    }
}

#[inline]
fn pack_pixel(color: (u8, u8, u8), pixel_format: PixelFormat) -> u32 {
    let (r, g, b) = color;
    match pixel_format {
        PixelFormat::Bgr => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
        _ => ((b as u32) << 16) | ((g as u32) << 8) | r as u32,
    }
}

fn clear_framebuffer(fb: &FramebufferInfo, color: (u8, u8, u8)) {
    let base = fb.base as *mut u32;
    let packed = pack_pixel(color, fb.pixel_format);

    unsafe {
        for y in 0..fb.height {
            for x in 0..fb.width {
                base.add(y * fb.stride + x).write_volatile(packed);
            }
        }
    }
}
