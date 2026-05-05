fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("flynt_mobile=info".parse().unwrap())
                .add_directive("flynt_store=info".parse().unwrap())
                .add_directive("flynt_core=info".parse().unwrap()),
        )
        .init();

    dioxus::LaunchBuilder::mobile()
        .launch(flynt_mobile::app::App);
}
