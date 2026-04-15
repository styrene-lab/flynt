use dioxus::desktop::{Config, LogicalSize, WindowBuilder};

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::default().with_window(
                WindowBuilder::new()
                    .with_title("Codex")
                    .with_inner_size(LogicalSize::new(1280.0f64, 860.0f64))
                    .with_min_inner_size(LogicalSize::new(800.0f64, 500.0f64))
                    .with_always_on_top(false)
                    .with_resizable(true),
            ),
        )
        .launch(codex_app::app::App);
}
