#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ObjectType {
    Kernel,
    Service,
    Process,
    Thread,
    Driver,
    Device,
    Timer,
    Event,
    Surface,
    Window,
    File,
    Directory,
    Volume,
    Filesystem,
    Mount,
    Socket,
    Pipe,
}

impl ObjectType {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjectType::Kernel => "Kernel",
            ObjectType::Service => "Service",
            ObjectType::Process => "Process",
            ObjectType::Thread => "Thread",
            ObjectType::Driver => "Driver",
            ObjectType::Device => "Device",
            ObjectType::Timer => "Timer",
            ObjectType::Event => "Event",
            ObjectType::Surface => "Surface",
            ObjectType::Window => "Window",
            ObjectType::File => "File",
            ObjectType::Directory => "Directory",
            ObjectType::Volume => "Volume",
            ObjectType::Filesystem => "Filesystem",
            ObjectType::Mount => "Mount",
            ObjectType::Socket => "Socket",
            ObjectType::Pipe => "Pipe",
        }
    }

    pub const fn code(self) -> u16 {
        match self {
            ObjectType::Kernel => 1,
            ObjectType::Service => 2,
            ObjectType::Process => 3,
            ObjectType::Thread => 4,
            ObjectType::Driver => 5,
            ObjectType::Device => 6,
            ObjectType::Timer => 7,
            ObjectType::Event => 8,
            ObjectType::Surface => 9,
            ObjectType::Window => 10,
            ObjectType::File => 11,
            ObjectType::Directory => 12,
            ObjectType::Volume => 13,
            ObjectType::Filesystem => 14,
            ObjectType::Mount => 15,
            ObjectType::Socket => 16,
            ObjectType::Pipe => 17,
        }
    }

    pub const fn prefix(self) -> &'static str {
        match self {
            ObjectType::Kernel => "KRN",
            ObjectType::Service => "SVC",
            ObjectType::Process => "PROC",
            ObjectType::Thread => "THR",
            ObjectType::Driver => "DRV",
            ObjectType::Device => "DEV",
            ObjectType::Timer => "TMR",
            ObjectType::Event => "EVT",
            ObjectType::Surface => "SUR",
            ObjectType::Window => "WND",
            ObjectType::File => "FIL",
            ObjectType::Directory => "DIR",
            ObjectType::Volume => "VOL",
            ObjectType::Filesystem => "FS",
            ObjectType::Mount => "MNT",
            ObjectType::Socket => "SOC",
            ObjectType::Pipe => "PIP",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        if input.eq_ignore_ascii_case("kernel") {
            Some(Self::Kernel)
        } else if input.eq_ignore_ascii_case("service") {
            Some(Self::Service)
        } else if input.eq_ignore_ascii_case("process") {
            Some(Self::Process)
        } else if input.eq_ignore_ascii_case("thread") {
            Some(Self::Thread)
        } else if input.eq_ignore_ascii_case("driver") {
            Some(Self::Driver)
        } else if input.eq_ignore_ascii_case("device") {
            Some(Self::Device)
        } else if input.eq_ignore_ascii_case("timer") {
            Some(Self::Timer)
        } else if input.eq_ignore_ascii_case("event") {
            Some(Self::Event)
        } else if input.eq_ignore_ascii_case("surface") {
            Some(Self::Surface)
        } else if input.eq_ignore_ascii_case("window") {
            Some(Self::Window)
        } else if input.eq_ignore_ascii_case("file") {
            Some(Self::File)
        } else if input.eq_ignore_ascii_case("directory") || input.eq_ignore_ascii_case("dir") {
            Some(Self::Directory)
        } else if input.eq_ignore_ascii_case("volume") {
            Some(Self::Volume)
        } else if input.eq_ignore_ascii_case("filesystem") || input.eq_ignore_ascii_case("fs") {
            Some(Self::Filesystem)
        } else if input.eq_ignore_ascii_case("mount") {
            Some(Self::Mount)
        } else if input.eq_ignore_ascii_case("socket") {
            Some(Self::Socket)
        } else if input.eq_ignore_ascii_case("pipe") {
            Some(Self::Pipe)
        } else {
            None
        }
    }
}
