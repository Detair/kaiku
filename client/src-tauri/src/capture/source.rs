//! Source Enumeration
//!
//! Discovers available monitors and windows via `scap`.

use scap::capturer::Options;
use tracing::debug;

use super::{CaptureError, CaptureSource, CaptureSourceType};

/// Enumerate all available capture sources (monitors and windows).
///
/// Returns at least one monitor on supported platforms.
/// Thumbnails are base64-encoded PNGs scaled to ~200px width.
pub fn enumerate_sources() -> Result<Vec<CaptureSource>, CaptureError> {
    if !scap::is_supported() {
        return Err(CaptureError::NotSupported);
    }

    if !scap::has_permission() {
        // On macOS, this triggers the system permission prompt
        if !scap::request_permission() {
            return Err(CaptureError::PermissionDenied);
        }
    }

    let targets = scap::get_all_targets();
    if targets.is_empty() {
        return Err(CaptureError::NoSources);
    }

    let mut sources = Vec::with_capacity(targets.len());

    for target in &targets {
        let source = match target {
            scap::Target::Display(display) => {
                let id = format!("display:{}", display.id);
                let name = if display.title.is_empty() {
                    format!("Display {}", display.id)
                } else {
                    display.title.clone()
                };

                debug!(id = %id, name = %name, "Found display source");

                CaptureSource {
                    id,
                    name,
                    source_type: CaptureSourceType::Monitor,
                    thumbnail: None, // Thumbnails generated on-demand to avoid blocking
                    is_primary: false, // scap doesn't expose primary display info
                }
            }
            scap::Target::Window(window) => {
                let id = format!("window:{}", window.id);
                let name = if window.title.is_empty() {
                    format!("Window {}", window.id)
                } else {
                    window.title.clone()
                };

                debug!(id = %id, name = %name, "Found window source");

                CaptureSource {
                    id,
                    name,
                    source_type: CaptureSourceType::Window,
                    thumbnail: None,
                    is_primary: false,
                }
            }
        };

        sources.push(source);
    }

    debug!(count = sources.len(), "Enumerated capture sources");
    Ok(sources)
}

/// Find the `scap::Target` matching a source ID string.
pub fn find_target_by_id(source_id: &str) -> Option<scap::Target> {
    let targets = scap::get_all_targets();

    for target in targets {
        let id = match &target {
            scap::Target::Display(d) => format!("display:{}", d.id),
            scap::Target::Window(w) => format!("window:{}", w.id),
        };
        if id == source_id {
            return Some(target);
        }
    }

    None
}

/// Build `scap::capturer::Options` for a given target at specified resolution and FPS.
pub fn build_capture_options(
    target: scap::Target,
    fps: u32,
    _output_width: u32,
    _output_height: u32,
) -> Options {
    let crop = None; // Full capture, encoder handles resolution

    Options {
        fps,
        target: Some(target),
        show_cursor: true,
        show_highlight: false,
        excluded_targets: None,
        output_type: scap::frame::FrameType::BGRAFrame,
        output_resolution: scap::capturer::Resolution::_1080p,
        crop_area: crop,
        ..Default::default()
    }
}
