use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use std::{borrow::Cow, path::PathBuf};
use wry::http::{Request as HttpRequest, Response as HttpResponse};

fn vault_root() -> PathBuf {
    std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|window| {
            if window[0] == "--vault" {
                Some(PathBuf::from(&window[1]))
            } else {
                None
            }
        })
        .or_else(|| std::env::var("CODEX_VAULT").map(PathBuf::from).ok())
        .unwrap_or_else(|| {
            dirs::document_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
                .join("Codyx")
        })
}

fn main() {
    let root = vault_root();

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::default()
                .with_menu(codex_app::menu::build_menu_bar())
                .with_disable_context_menu(false)
                .with_window(
                    WindowBuilder::new()
                        .with_title("Codyx")
                        .with_inner_size(LogicalSize::new(1280.0f64, 860.0f64))
                        .with_min_inner_size(LogicalSize::new(800.0f64, 500.0f64))
                        .with_always_on_top(false)
                        .with_resizable(true),
                )
                // Serve vault files at vault://localhost/<rel-path>
                // Allows <img src="vault://localhost/image.png"> to resolve.
                .with_custom_protocol("vault", {
                    let root = root.clone();
                    move |_id, request: HttpRequest<Vec<u8>>| {
                        let path_str = request.uri().path().trim_start_matches('/');
                        // URL-decode %20 etc.
                        let decoded = percent_decode(path_str);
                        let abs = root.join(&decoded);

                        match std::fs::read(&abs) {
                            Ok(bytes) => {
                                let mime = mime_type(&abs);
                                HttpResponse::builder()
                                    .header("Content-Type", mime)
                                    .header("Access-Control-Allow-Origin", "*")
                                    .body(Cow::Owned(bytes))
                                    .unwrap()
                            }
                            Err(_) => HttpResponse::builder()
                                .status(404)
                                .body(Cow::Borrowed(b"not found" as &[u8]))
                                .unwrap(),
                        }
                    }
                }),
        )
        .launch(codex_app::app::App);
}

/// Minimal URL percent-decoder (handles %20, %2F, etc.).
fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut bytes = s.bytes().peekable();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let h1 = bytes.next().unwrap_or(b'0');
            let h2 = bytes.next().unwrap_or(b'0');
            if let Ok(c) = u8::from_str_radix(
                std::str::from_utf8(&[h1, h2]).unwrap_or("00"),
                16,
            ) {
                out.push(c as char);
                continue;
            }
        }
        out.push(b as char);
    }
    out
}

fn mime_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png")              => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif")              => "image/gif",
        Some("svg")              => "image/svg+xml",
        Some("webp")             => "image/webp",
        Some("pdf")              => "application/pdf",
        Some("mp4")              => "video/mp4",
        Some("css")              => "text/css",
        Some("js")               => "application/javascript",
        _                        => "application/octet-stream",
    }
}
