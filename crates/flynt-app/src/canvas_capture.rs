//! Canvas capture pipeline — gives the agent eyes on what the user actually
//! sees in the viewport.
//!
//! Rust crates over JS bundles: `xcap` is the canonical cross-platform
//! capture crate (Linux X11/Wayland + macOS, single dep, no JS toolchain).
//!
//! ## Architecture
//!
//! Tool side (`omegon-design::canvas_capture_viewport`) writes a request file
//! to `<vault>/.flynt-local/flynt/capture-requests/<id>.json`. Flynt-app's
//! `CanvasView` watches that directory via the existing vault watcher; on
//! detection, it:
//!   1. Queries each cell's iframe via `postMessage` for its body's natural
//!      width/height (the "content_box"). The response listener is injected
//!      into every iframe's bootstrap script — see `inject_measurement_hook`
//!      in `views::canvas`.
//!   2. Computes the canvas-pane's screen-relative bounds.
//!   3. Calls `xcap` to capture the Flynt window, crops to those bounds.
//!   4. Writes PNG + response JSON to `<vault>/.flynt-local/flynt/capture-responses/`.
//!
//! Tool polls for the response file (5s timeout), returns image + metrics.
//!
//! ## Permissions
//!
//! macOS: first call triggers the system Screen Recording permission prompt.
//! `permission_status()` probes whether we already have it (heuristic; the
//! authoritative check is "did the capture call return a non-blank image")
//! and lets the skill surface guidance to the operator before the first call.
//! Linux: no special permissions for X11/Wayland (process must be able to
//! connect to the display server, which any GUI process already does).

use serde::{Deserialize, Serialize};
use std::path::Path;

// Wire types live in flynt-core so the omegon-design tool (separate binary)
// can construct/parse them too. Re-exported here for ergonomic local use.
pub use flynt_core::canvas::{
    capture_request_dir, capture_response_dir, BoxXywh, CaptureRequest, CaptureResponse,
    CellMetric,
};

/// Probe whether macOS Screen Recording permission is granted. Heuristic:
/// attempt a minimal capture and check for a black/blank result. On Linux
/// this always returns granted. Used by the skill to surface guidance to
/// the operator before the first real capture call.
pub fn permission_status() -> PermissionStatus {
    #[cfg(target_os = "macos")]
    {
        // Try to enumerate windows. xcap surfaces a permissions error here
        // when Screen Recording isn't granted on macOS — capture_image()
        // returns a blank image rather than erroring, but Window::all()
        // includes all windows regardless. The actual permission check we
        // can do cheaply: capture the primary monitor at 1×1 and look at
        // the result. A blank-black 1px image strongly indicates the OS
        // is blocking us at the compositor level.
        match xcap::Monitor::all() {
            Ok(monitors) => {
                if let Some(m) = monitors.first() {
                    match m.capture_region(0, 0, 1, 1) {
                        Ok(img) => {
                            // RgbaImage; check pixel 0,0
                            let p = img.get_pixel(0, 0);
                            // True black with full alpha is the macOS
                            // blocked-by-permission signal. Real screens
                            // are ~never pure black; rgb sum > 0 means we
                            // probably have the permission.
                            let visible = (p[0] as u32) + (p[1] as u32) + (p[2] as u32) > 0;
                            if visible {
                                PermissionStatus::Granted
                            } else {
                                PermissionStatus::Denied {
                                    instructions: "Grant via System Settings → Privacy & Security → Screen Recording → enable Flynt. Restart Flynt after granting.".into(),
                                }
                            }
                        }
                        Err(e) => PermissionStatus::Denied {
                            instructions: format!("xcap capture failed: {e}. Check System Settings → Privacy & Security → Screen Recording."),
                        },
                    }
                } else {
                    PermissionStatus::Unknown { detail: "no monitors enumerated".into() }
                }
            }
            Err(e) => PermissionStatus::Unknown { detail: format!("monitor enumeration failed: {e}") },
        }
    }
    #[cfg(target_os = "linux")]
    {
        // X11/Wayland: any GUI process can capture its own window without
        // additional permission. We still surface a status so the tool
        // surface looks identical across platforms.
        PermissionStatus::Granted
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        PermissionStatus::Unknown { detail: "unsupported platform".into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PermissionStatus {
    Granted,
    Denied { instructions: String },
    Unknown { detail: String },
}

/// Find Flynt's main window in xcap's window list. Linux/macOS: matches by
/// title prefix "Flynt". Returns None if no match — caller should surface
/// the failure rather than silently capturing the wrong window.
pub fn find_flynt_window() -> Option<xcap::Window> {
    let windows = xcap::Window::all().ok()?;
    windows.into_iter().find(|w| {
        w.title().map(|t| t.starts_with("Flynt")).unwrap_or(false)
            && !w.is_minimized().unwrap_or(true)
    })
}

/// Capture the full Flynt window and crop to the given window-relative
/// rect. Returns (PNG bytes, image width, image height). The image is
/// PNG-encoded so we can write it to disk and base64-inline the same bytes
/// without re-encoding.
pub fn capture_pane(
    window_relative_bounds: BoxXywh,
) -> anyhow::Result<(Vec<u8>, u32, u32)> {
    use anyhow::Context;
    let window = find_flynt_window().context("Flynt window not found in xcap window list")?;
    let img = window.capture_image().context("xcap capture_image failed")?;

    // Crop to the bounds. Bounds are window-logical coords; xcap returns the
    // full window image at native resolution. Scale factor lives on the
    // monitor, not the window — query the window's current monitor.
    let scale = window
        .current_monitor()
        .ok()
        .and_then(|m| m.scale_factor().ok())
        .unwrap_or(1.0)
        .max(0.0001);
    let crop_x = (window_relative_bounds.x * scale).max(0.0) as u32;
    let crop_y = (window_relative_bounds.y * scale).max(0.0) as u32;
    let crop_w = (window_relative_bounds.w * scale).max(1.0) as u32;
    let crop_h = (window_relative_bounds.h * scale).max(1.0) as u32;

    // Clamp against the actual image dimensions — defends against a JS-side
    // bounding rect that includes off-window pixels (negative or oversized).
    let img_w = img.width();
    let img_h = img.height();
    let x = crop_x.min(img_w.saturating_sub(1));
    let y = crop_y.min(img_h.saturating_sub(1));
    let w = crop_w.min(img_w.saturating_sub(x));
    let h = crop_h.min(img_h.saturating_sub(y));

    let cropped = xcap::image::imageops::crop_imm(&img, x, y, w, h).to_image();

    // Encode to PNG in memory.
    let mut bytes = Vec::with_capacity((w as usize) * (h as usize) * 4 / 3);
    cropped
        .write_to(&mut std::io::Cursor::new(&mut bytes), xcap::image::ImageFormat::Png)
        .context("PNG encode failed")?;

    Ok((bytes, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_xywh_round_trips_through_json() {
        let b = BoxXywh { x: 10.0, y: 20.0, w: 300.0, h: 200.0 };
        let json = serde_json::to_string(&b).unwrap();
        let back: BoxXywh = serde_json::from_str(&json).unwrap();
        assert_eq!(back.x, b.x);
        assert_eq!(back.h, b.h);
    }

    #[test]
    fn capture_request_default_include_metrics_is_true() {
        let json = r#"{"request_id":"x"}"#;
        let req: CaptureRequest = serde_json::from_str(json).unwrap();
        assert!(req.include_metrics);
    }

    #[test]
    fn permission_status_is_serializable_each_variant() {
        let g = PermissionStatus::Granted;
        let d = PermissionStatus::Denied { instructions: "x".into() };
        let u = PermissionStatus::Unknown { detail: "y".into() };
        for s in [g, d, u] {
            let json = serde_json::to_string(&s).unwrap();
            assert!(json.contains("status"));
        }
    }
}
