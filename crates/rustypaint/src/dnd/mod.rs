#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;

// winit reports dropped files on every backend except Wayland, which has to be followed by hand.
#[cfg(all(unix, not(target_os = "macos")))]
pub use wayland::{drops, watch};

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn watch(_window: &dyn iced::window::Window) {}

#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn drops() -> iced::Subscription<std::path::PathBuf> {
    iced::Subscription::none()
}
