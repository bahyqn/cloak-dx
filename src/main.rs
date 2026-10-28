use dioxus::desktop::{LogicalSize, WindowBuilder};

use crate::app::App;

mod app;
mod components;
mod i18n;
mod services;
mod theme;

fn main() {
    // dioxus::launch(app::App);
    dioxus::LaunchBuilder::new()
        .with_cfg(
            dioxus::desktop::Config::default()
                .with_menu(None)
                .with_window(
                    WindowBuilder::new()
                        .with_maximized(false)
                        .with_title("cloak-dx")
                        .with_inner_size(LogicalSize::new(1513.0, 900.0))
                        .with_min_inner_size(LogicalSize::new(1513.0, 900.0)),
                ),
        )
        .launch(App);
}
