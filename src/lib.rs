pub mod domain;
pub mod wslc;
pub mod config;
pub mod app;
pub mod tray;
pub mod rcompose;
pub mod settings;
pub mod single_instance;

/// Process creation flag that keeps console child processes from opening a window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

slint::include_modules!();
