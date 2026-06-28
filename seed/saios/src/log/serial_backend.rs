pub fn init() {
    crate::drivers::serial::init();
}

pub fn write_str(s: &str) {
    crate::drivers::serial::write_str(s);
}

pub fn write_fmt(args: core::fmt::Arguments<'_>) {
    crate::drivers::serial::write_fmt(args);
}
