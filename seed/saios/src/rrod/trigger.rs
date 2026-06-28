use super::capture;
use super::context::{self, RRodContext};
use super::{freeze, renderer, serial};

pub fn trigger(context: RRodContext) -> ! {
    freeze::freeze_other_cpus();
    serial::flush();
    serial::dump_report(&context);

    if let Some(boot_info) = context::boot_info() {
        renderer::render(boot_info, &context);
    }

    freeze::halt_forever()
}

pub fn fatal(reason: &'static str) -> ! {
    trigger(capture::from_fatal(reason))
}

#[macro_export]
macro_rules! rrod {
    ($reason:expr) => {{ $crate::rrod::fatal($reason) }};
}
