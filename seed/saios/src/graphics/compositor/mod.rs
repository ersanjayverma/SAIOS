pub mod desktop;
pub mod window_manager;

pub use desktop::DesktopCompositor;
pub use window_manager::{Window, WindowManager, MAX_WINDOWS};
