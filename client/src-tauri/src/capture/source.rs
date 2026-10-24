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

/// Build `scap::capturer::Options` for a given target at the given FPS.
///
/// `max_width` x `max_height` is the tier's largest encode size. Backends
/// that can scale natively (macOS) are asked for the matching resolution;
/// on the others frames arrive at the source's own size and the converter
/// scales them (see `convert::encode_dimensions`).
pub fn build_capture_options(
    target: scap::Target,
    fps: u32,
    max_width: u32,
    max_height: u32,
) -> Options {
    Options {
        fps,
        target: Some(target),
        show_cursor: true,
        show_highlight: false,
        excluded_targets: None,
        output_type: scap::frame::FrameType::BGRAFrame,
        output_resolution: output_resolution(max_width, max_height),
        crop_area: None,
        ..Default::default()
    }
}

/// Smallest `scap` resolution that covers a `max_width` x `max_height` tier.
///
/// `scap` never upscales with it (it takes the minimum of this and the
/// source size), so a small window keeps its own size.
const fn output_resolution(max_width: u32, max_height: u32) -> scap::capturer::Resolution {
    use scap::capturer::Resolution;
    if max_width <= 640 && max_height <= 480 {
        Resolution::_480p
    } else if max_width <= 1280 && max_height <= 720 {
        Resolution::_720p
    } else {
        Resolution::_1080p
    }
}

#[cfg(test)]
mod tests {
    use scap::capturer::Resolution;

    use super::*;

    #[test]
    fn output_resolution_covers_tier() {
        assert!(matches!(output_resolution(854, 480), Resolution::_720p));
        assert!(matches!(output_resolution(1280, 720), Resolution::_720p));
        assert!(matches!(output_resolution(1920, 1080), Resolution::_1080p));
        assert!(matches!(output_resolution(320, 240), Resolution::_480p));
    }
}
