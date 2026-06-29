pub struct Seed {
    pub boot_info: *const SaiosBootInfo,
}
impl Seed {
    pub fn init(boot_info: *const SaiosBootInfo) -> Self {
        Self { boot_info }
    }
}