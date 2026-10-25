//! ritz-get-gui: a Graphite-styled egui front end that only ever drives `ritz-get`.
//!
//! Module map (I1 scaffold; owners in brackets are the plan rows that fill the stubs):
//! - `model`     shared types: listing, `Command`, `Action`, UI enums            [I1, frozen]
//! - `theme`     Graphite palette, fonts, pills, pane helpers                     [I1]
//! - `porcelain` `list --porcelain` parser                                        [I2]
//! - `runner`    one-at-a-time command runner                                     [I3]
//! - `icons`     icon sources, cache, fetch, draw                                 [I6]
//! - `app`       `AppState` + `App` (eframe), dispatch                            [I7]
//! - `ui`        frame layout [I1] + panes topbar/groups/side/bottom/settings/modals [I8-I13]

mod app;
mod icons;
mod model;
mod porcelain;
mod runner;
#[cfg(test)]
mod sample;
mod theme;
mod ui;

/// The user's 128 px GUI icon (copied artwork, D51): the window icon.
const WINDOW_ICON: &[u8] = include_bytes!("../assets/icons/icon-ritz-get-gui-128.png");

fn main() -> eframe::Result {
    let icon = image::load_from_memory(WINDOW_ICON).ok().map(|img| {
        let img = img.into_rgba8();
        egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("ritz-get-gui") // D42
        .with_app_id("ritz-get-gui") // Wayland app_id / X11 class, matches the .desktop file
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([520.0, 420.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("ritz-get-gui", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
