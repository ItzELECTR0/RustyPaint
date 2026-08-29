#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod assets;
mod canvas;
mod config;
mod dnd;
mod doc;
mod gpu;
mod paint;
mod select;
mod text;
mod ui;

use app::{App, Message};
use iced::{Element, Renderer, Theme};

fn view(app: &App) -> Element<'_, Message, Theme, Renderer> {
    app.view()
}

fn main() -> iced::Result {
    let (config, _) = config::boot();

    iced::application(App::new, App::update, view)
        .subscription(App::subscription)
        .title(App::title)
        .theme(App::theme)
        .scale_factor(|_| app::UI_SCALE)
        .font(assets::UI_FONT)
        .default_font(assets::ui_font())
        .window(iced::window::Settings {
            min_size: Some(iced::Size::new(640.0, 480.0)),
            icon: window_icon(),
            decorations: config.decorations,
            transparent: true,
            ..Default::default()
        })
        .window_size(iced::Size::new(1280.0, 800.0))
        .exit_on_close_request(false)
        .run()
}

fn window_icon() -> Option<iced::window::Icon> {
    let img = image::load_from_memory(assets::APP_ICON_PNG)
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    iced::window::icon::from_rgba(img.into_raw(), w, h).ok()
}
