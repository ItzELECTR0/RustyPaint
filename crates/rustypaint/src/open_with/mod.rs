use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod macos;

// Every desktop but macOS hands the file over on the command line.
pub fn first() -> Option<PathBuf> {
    std::env::args_os().nth(1).map(PathBuf::from)
}

// macOS sends an Apple Event instead, which arrives after the window is already up.
#[cfg(target_os = "macos")]
pub use macos::{later, watch};

#[cfg(not(target_os = "macos"))]
pub fn watch(_window: &dyn iced::window::Window) {}

#[cfg(not(target_os = "macos"))]
pub fn later() -> iced::Subscription<PathBuf> {
    iced::Subscription::none()
}
