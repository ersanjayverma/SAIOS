#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ObjectType {
    Kernel,
    Process,
    Thread,
    Driver,
    Device,
    Mount,
}

impl ObjectType {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectType::Kernel => "Kernel",
            ObjectType::Process => "Process",
            ObjectType::Thread => "Thread",
            ObjectType::Driver => "Driver",
            ObjectType::Device => "Device",
            ObjectType::Mount => "Mount",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        if input.eq_ignore_ascii_case("kernel") {
            Some(Self::Kernel)
        } else if input.eq_ignore_ascii_case("process") {
            Some(Self::Process)
        } else if input.eq_ignore_ascii_case("thread") {
            Some(Self::Thread)
        } else if input.eq_ignore_ascii_case("driver") {
            Some(Self::Driver)
        } else if input.eq_ignore_ascii_case("device") {
            Some(Self::Device)
        } else if input.eq_ignore_ascii_case("mount") {
            Some(Self::Mount)
        } else {
            None
        }
    }
}
