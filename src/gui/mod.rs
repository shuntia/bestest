// GUI ENTRY POINT
slint::include_modules!();

pub mod app;

/// Initializes and sets the configuration up
pub fn init() {
    app::launch();
}
