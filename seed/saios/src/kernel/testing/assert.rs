#[macro_export]
macro_rules! kt_assert {
    ($cond:expr) => {
        if !($cond) {
            return Err(concat!("assert failed: ", stringify!($cond)));
        }
    };
}

#[macro_export]
macro_rules! kt_assert_eq {
    ($expected:expr, $actual:expr) => {
        if $expected != $actual {
            return Err(concat!(
                "assert_eq failed: ",
                stringify!($expected),
                " != ",
                stringify!($actual)
            ));
        }
    };
}

#[macro_export]
macro_rules! kt_fail {
    ($msg:expr) => {
        return Err($msg);
    };
}
