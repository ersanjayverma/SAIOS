use efi_main::SaiosBootInfo;
use efi_main::graphics::FramebufferInfo;

use crate::diagnostics;
use crate::graphics::backbuffer::BackBuffer;
use crate::graphics::compositor::DesktopCompositor;
use crate::graphics::contracts::Renderer;
use crate::graphics::fonts::bitmap::BitmapFont;
use crate::graphics::images::bmp;
use crate::graphics::software::renderer::SoftwareRenderer;
use crate::graphics::{Color, Point};
use crate::rrod;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_mid: 0,
            offset_high: 0,
            zero: 0,
        }
    }

    fn set_handler(&mut self, handler: u64, selector: u16) {
        self.offset_low = handler as u16;
        self.selector = selector;
        self.ist = 0;
        self.attributes = 0x8E;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.zero = 0;
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];
static mut BMP_RGBA_SCRATCH: [u8; 1024 * 768 * 4] = [0; 1024 * 768 * 4];

const SPLASH_BMP: &[u8] = include_bytes!("../../../../boot/uefi/efi_main/src/assets/splash.bmp");

unsafe extern "C" {
    fn seed_isr_ud();
    fn seed_isr_gp();
    fn seed_isr_pf();
}

core::arch::global_asm!(
    r#"
    .global seed_isr_ud
seed_isr_ud:
    cli
    mov rdi, rsp
    mov esi, 6
    xor edx, edx
    call seed_exception_from_stack
1:
    hlt
    jmp 1b

    .global seed_isr_gp
seed_isr_gp:
    cli
    mov rdi, rsp
    mov esi, 13
    mov edx, 1
    call seed_exception_from_stack
2:
    hlt
    jmp 2b

    .global seed_isr_pf
seed_isr_pf:
    cli
    mov rdi, rsp
    mov esi, 14
    mov edx, 1
    call seed_exception_from_stack
3:
    hlt
    jmp 3b
"#
);

#[unsafe(no_mangle)]
extern "C" fn seed_exception_from_stack(stack: *const u64, vector: u32, has_error_code: u32) -> ! {
    diagnostics::exception_trap(vector);
    let context = rrod::capture::from_exception_stack(stack, vector, has_error_code != 0);
    rrod::trigger(context)
}

pub fn init(boot_info: *const SaiosBootInfo) {
    diagnostics::init_serial();
    diagnostics::stage("SEED init entry");

    rrod::set_boot_info(boot_info);

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
}

impl<'a> KernelInit<'a> {
    fn new(boot_info: &'a SaiosBootInfo) -> Self {
        Self {
            boot_info,
            fb: boot_info.framebuffer,
            backbuffer: None,
        }
    }

    fn stage0_cpu(&mut self) {
        clear_framebuffer(&self.fb, (0, 0, 0));
        checkpoint(&self.fb, 0, (255, 255, 255));
    }

    fn stage1_exceptions(&mut self) {
        install_exception_handlers();
        checkpoint(&self.fb, 1, (255, 180, 40));
    }

    fn stage2_memory(&mut self) {
        checkpoint(&self.fb, 2, (80, 220, 120));
    }

    fn stage3_drivers(&mut self) {
        diagnostics::stage("Stage 3 Drivers");
        checkpoint(&self.fb, 3, (90, 140, 255));
    }

    fn stage4_logging(&mut self) {
        diagnostics::init_logger();
        diagnostics::stage("Stage 4 Logging");
        checkpoint(&self.fb, 4, (255, 70, 70));
    }

    fn stage5_graphics(&mut self) {
        diagnostics::stage("Stage 5 Graphics");

        self.backbuffer = Some(BackBuffer::new(
            self.fb.width as u32,
            self.fb.height as u32,
            self.fb.stride,
            self.fb.bpp as u8,
        ));

        let backbuffer = self.backbuffer.as_mut().unwrap();
        let mut surface = backbuffer.surface();
        let mut renderer = SoftwareRenderer::from_surface(&mut surface);
        let font = BitmapFont::new_5x7();

        renderer.clear(Color::rgb(10, 14, 30));
        renderer.draw_text(
            Point { x: 10, y: 10 },
            "SEED",
            &font,
            Color::rgb(255, 255, 255),
        );

        if let Ok(decoded) = bmp::decode(SPLASH_BMP) {
            diagnostics::stage("BMP decoded");
            let scratch_ptr = core::ptr::addr_of_mut!(BMP_RGBA_SCRATCH).cast::<u8>();
            let scratch = unsafe { core::slice::from_raw_parts_mut(scratch_ptr, 1024 * 768 * 4) };
            if let Ok(image) = decoded.as_image(scratch) {
                renderer.draw_image(&image, Point { x: 0, y: 0 });
                diagnostics::stage("BMP draw complete");
            }
        }

        checkpoint(&self.fb, 5, (255, 130, 40));
    }

    fn stage6_desktop(&mut self) {
        diagnostics::stage("Stage 6 Desktop");

        if let Some(backbuffer) = self.backbuffer.as_mut() {
            let mut surface = backbuffer.surface();
            let mut renderer = SoftwareRenderer::from_surface(&mut surface);
            let font = BitmapFont::new_5x7();

            let mut desktop = DesktopCompositor::new(Color::rgb(18, 24, 48));
            desktop.seed_demo_windows();
            desktop.compose_with_renderer(&mut renderer);
            renderer.draw_text(
                Point { x: 12, y: 12 },
                "SEED",
                &font,
                Color::rgb(250, 250, 255),
            );
        }

        checkpoint(&self.fb, 6, (255, 190, 30));
    }

    fn stage7_runtime(&mut self) {
        diagnostics::stage("Stage 7 Runtime");

        if let Some(backbuffer) = self.backbuffer.as_ref() {
            backbuffer.blit_to_framebuffer(self.boot_info.framebuffer.base as *mut u8);
        }

        checkpoint(&self.fb, 7, (80, 220, 255));
        diagnostics::stage("Framebuffer blit complete");
    }
}

fn install_exception_handlers() {
    let mut cs: u16;
    unsafe {
        core::arch::asm!("mov {0:x}, cs", out(reg) cs, options(nomem, nostack, preserves_flags));
    }

    unsafe {
        IDT[6].set_handler(seed_isr_ud as *const () as usize as u64, cs);
        IDT[13].set_handler(seed_isr_gp as *const () as usize as u64, cs);
        IDT[14].set_handler(seed_isr_pf as *const () as usize as u64, cs);

        let idtr = Idtr {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: core::ptr::addr_of!(IDT) as u64,
        };

        core::arch::asm!("lidt [{}]", in(reg) &idtr, options(readonly, nostack, preserves_flags));
    }
}

#[inline]
fn pack_bgr(color: (u8, u8, u8)) -> u32 {
    ((color.2 as u32) << 16) | ((color.1 as u32) << 8) | color.0 as u32
}

fn checkpoint(fb: &FramebufferInfo, index: usize, color: (u8, u8, u8)) {
    let base = fb.base as *mut u32;
    let x0 = 8 + index * 10;
    let y0 = 8usize;
    let c = pack_bgr(color);

    unsafe {
        for y in y0..(y0 + 6) {
            for x in x0..(x0 + 6) {
                if x < fb.width && y < fb.height {
                    base.add(y * fb.stride + x).write_volatile(c);
                }
            }
        }
    }
}

fn clear_framebuffer(fb: &FramebufferInfo, color: (u8, u8, u8)) {
    let base = fb.base as *mut u32;
    let packed = pack_bgr(color);

    unsafe {
        for y in 0..fb.height {
            for x in 0..fb.width {
                base.add(y * fb.stride + x).write_volatile(packed);
            }
        }
    }
}
