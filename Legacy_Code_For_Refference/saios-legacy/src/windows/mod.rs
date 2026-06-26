pub mod ntdll;
pub mod pe_loader;
pub mod syscall;

pub fn init() {
    if crate::compatibility_contract::CompatibilityContract::require_layer(
        crate::compatibility_contract::CompatibilityLayer::WindowsCompatibility,
    )
    .is_err()
    {
        crate::serial_println!(
            "Windows compatibility layer scaffold present; roadmap Phase 6 required"
        );
        return;
    }
    crate::serial_println!("Initializing Windows compatibility layer");
    ntdll::init();
}
