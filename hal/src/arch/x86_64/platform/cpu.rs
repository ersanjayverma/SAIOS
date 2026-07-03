use crate::arch::x86_64::cpuid;
pub fn init() {
    let features = cpuid::features();
    // Some devices and VMs expose a reduced feature set. Keep booting and let
    // downstream subsystems choose fallbacks when possible.
    if !features.apic {
        crate::arch::x86_64::console::_print(format_args!(
            "cpu: warning: APIC not reported; interrupt controller features may be limited\n"
        ));
    }

    if !features.msr {
        crate::arch::x86_64::console::_print(format_args!(
            "cpu: warning: MSR not reported; advanced CPU controls disabled\n"
        ));
    }

    if !features.tsc {
        crate::arch::x86_64::console::_print(format_args!(
            "cpu: warning: TSC not reported; timer precision may be reduced\n"
        ));
    }
}
