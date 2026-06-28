pub fn init() {
    crate::console::init_serial();
}

pub fn write_str(s: &str) {
    crate::console::write_str(s);
}

pub fn write_fmt(args: core::fmt::Arguments<'_>) {
    crate::console::write_fmt(args);
}
