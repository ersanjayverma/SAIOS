use crate::arch::x86_64::cpuid;
pub fn init() {
    let features = cpuid::features();
    // Verify required features
    if !features.apic {
        panic!("CPU does not support APIC");
    }

    if !features.msr {
        panic!("CPU does not support MSRs");
    }

    if !features.tsc {
        panic!("CPU does not support TSC");
    }
}
