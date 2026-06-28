#![no_std]
#![no_main]

pub mod arch;
pub mod boot;
pub mod drivers;
pub mod fs;
pub mod graphics;
pub mod ipc;
pub mod log;
pub mod memory;
pub mod net;
pub mod process;
pub mod rrod;
pub mod scheduler;

use efi_main::SaiosBootInfo;
use graphics::backbuffer::BackBuffer;
use graphics::compositor::DesktopCompositor;
use graphics::contracts::Renderer;
use graphics::fonts::bitmap::BitmapFont;
use graphics::images::bmp;
use graphics::software::renderer::SoftwareRenderer;
use graphics::{Color, Point};

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
const SPLASH_BMP: &[u8] = include_bytes!("../../../boot/uefi/efi_main/src/assets/splash.bmp");

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

#[unsafe(no_mangle)]
extern "C" fn seed_exception_from_stack(stack: *const u64, vector: u32, has_error_code: u32) -> ! {
    drivers::serial::write_str("[SEED] exception trap\n");
    let context = rrod::capture::from_exception_stack(stack, vector, has_error_code != 0);
    rrod::trigger(context)
}

#[inline]
fn pack_bgr(color: (u8, u8, u8)) -> u32 {
    ((color.2 as u32) << 16) | ((color.1 as u32) << 8) | color.0 as u32
}

fn checkpoint(fb: &efi_main::graphics::FramebufferInfo, index: usize, color: (u8, u8, u8)) {
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

fn clear_framebuffer(fb: &efi_main::graphics::FramebufferInfo, color: (u8, u8, u8)) {
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start(boot_info: *const SaiosBootInfo) -> ! {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }

    rrod::set_boot_info(boot_info);

    install_exception_handlers();

    let fb = unsafe { (*boot_info).framebuffer };
    clear_framebuffer(&fb, (0, 0, 0));
    checkpoint(&fb, 0, (255, 255, 255));

    drivers::serial::init();
    drivers::serial::write_str("[SEED] K0 serial init\n");
    checkpoint(&fb, 1, (255, 180, 40));

    log::logger::init();
    drivers::serial::write_str("[SEED] K1 logger init\n");
    checkpoint(&fb, 2, (80, 220, 120));

    log::info!("SEED started");
    checkpoint(&fb, 3, (90, 140, 255));

    log::info!("Framebuffer acquired");

    let mut backbuffer = BackBuffer::new(fb.width as u32, fb.height as u32, fb.stride, fb.bpp as u8);
    log::debug!("Backbuffer initialized");
    {
        let mut surface = backbuffer.surface();
        let mut renderer = SoftwareRenderer::from_surface(&mut surface);
        let font = BitmapFont::new_5x7();
        log::debug!("Software renderer initialized");

        renderer.clear(Color::rgb(10, 14, 30));
        renderer.draw_text(
            Point { x: 10, y: 10 },
            "SEED",
            &font,
            Color::rgb(255, 255, 255),
        );

        if let Ok(decoded) = bmp::decode(SPLASH_BMP) {
            let _ = decoded;
            log::info!("BMP decoded");
            drivers::serial::write_str("[SEED] K4 bmp decode ok\n");
            checkpoint(&fb, 4, (255, 70, 70));

            let scratch_ptr = core::ptr::addr_of_mut!(BMP_RGBA_SCRATCH).cast::<u8>();
            let scratch = unsafe {
                core::slice::from_raw_parts_mut(scratch_ptr, 1024 * 768 * 4)
            };
            drivers::serial::write_str("[SEED] K5 before as_image\n");
            if let Ok(image) = decoded.as_image(scratch) {
                drivers::serial::write_str("[SEED] K6 as_image ok\n");
                checkpoint(&fb, 5, (255, 130, 40));

                renderer.draw_image(&image, Point { x: 0, y: 0 });
                drivers::serial::write_str("[SEED] K7 draw_image ok\n");
                checkpoint(&fb, 6, (255, 190, 30));
                log::debug!("BMP drawn to backbuffer");
            } else {
                drivers::serial::write_str("[SEED] K6 as_image failed\n");
            }
        } else {
            drivers::serial::write_str("[SEED] K4 bmp decode failed\n");
        }

        drivers::serial::write_str("[SEED] K8 before desktop compose\n");
        let mut desktop = DesktopCompositor::new(Color::rgb(18, 24, 48));
        desktop.seed_demo_windows();
        desktop.compose_with_renderer(&mut renderer);
        drivers::serial::write_str("[SEED] K9 desktop compose ok\n");
        checkpoint(&fb, 7, (120, 220, 120));

        renderer.draw_text(
            Point { x: 12, y: 12 },
            "SEED",
            &font,
            Color::rgb(250, 250, 255),
        );
        drivers::serial::write_str("[SEED] K10 final text ok\n");
    }

    drivers::serial::write_str("[SEED] K11 before blit\n");
    backbuffer.blit_to_framebuffer(fb.base as *mut u8);
    checkpoint(&fb, 8, (80, 220, 255));
    drivers::serial::write_str("[SEED] K12 blit ok\n");
    log::info!("Framebuffer blit complete");

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    drivers::serial::write_str("[SEED] panic entered\n");

    let context = rrod::capture::from_panic(info);
    rrod::trigger(context)
}
