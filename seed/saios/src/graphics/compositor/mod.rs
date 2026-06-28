pub mod desktop;
pub mod window_manager;

pub use desktop::DesktopCompositor;
pub use window_manager::{MAX_WINDOWS, Window, WindowManager};
