// GUI ENTRY POINT
slint::include_modules!();

pub mod app;

/// Initializes and sets the configuration up
pub async fn init() {
    app::launch().await;
}
