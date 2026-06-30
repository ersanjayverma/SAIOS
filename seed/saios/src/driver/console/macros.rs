#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ({
        $crate::driver::serial::_print(format_args!($($arg)*));
    });
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n");
    };

    ($fmt:expr) => {
        $crate::print!(concat!($fmt, "\n"));
    };

    ($fmt:expr, $($arg:tt)*) => {
        $crate::print!(concat!($fmt, "\n"), $($arg)*);
    };
}
