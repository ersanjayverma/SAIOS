#![no_std]
#![no_main]
use uefi::*;
use uefi::println;
mod pixelformat;
mod graphics;

#[entry]
fn main() -> Status {
     uefi::helpers::init().unwrap();

    println!("================================");
    println!("        SAIOS Bootloader"        );
    println!("================================");
    
    //firmware::print();

    
    let framebuffer = graphics::initialize().unwrap();
    println!("Framebuffer base address: {:#x}", framebuffer.base);
    println!("Framebuffer size: {} bytes", framebuffer.size);
    println!("Framebuffer stride: {} pixels", framebuffer.stride);
    println!("Framebuffer pixel format: {:?}", framebuffer.pixel_format);
    println!(
        "{}x{}",
        framebuffer.width,
        framebuffer.height
    );

    Status::SUCCESS
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}