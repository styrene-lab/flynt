fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("codex_mobile=info,codex_store=info")
        .init();

    dioxus::LaunchBuilder::mobile()
        .launch(codex_mobile::app::App);
}
