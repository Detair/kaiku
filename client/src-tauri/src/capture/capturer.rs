//! Frame Capturer
//!
//! Wraps `scap::Capturer` to produce I420 frames at the target FPS.
//! Runs on a background thread and sends frames via `mpsc::Sender`.
//!
//! Frames are converted at their actual size and row stride and scaled down
//! to fit the quality tier (`width` x `height` are the maximum, not a fixed
//! size), so a small window or a 4K monitor works on every tier.

use std::sync::mpsc;

use tokio::sync::watch;
use tracing::{debug, error, info, warn};

use super::convert::{infer_row_stride, I420Converter, PixelLayout};
use super::source::{build_capture_options, find_target_by_id};
use super::{CaptureError, I420Frame};

/// Frame capturer that produces I420 frames from a native capture source.
pub struct FrameCapturer {
    source_id: String,
    fps: u32,
    width: u32,
    height: u32,
}

/// Pixel data of one captured video frame, borrowed from the `scap` frame.
struct PackedFrame<'a> {
    layout: PixelLayout,
    width: i32,
    height: i32,
    data: &'a [u8],
}

/// Map a `scap` video frame to its packed pixel layout, if supported.
///
/// macOS and Windows deliver BGRA; the Linux `PipeWire` backend delivers
/// whatever format was negotiated (`BGRx`, `RGBx`, `xBGR` or `RGB`).
fn packed_frame(frame: &scap::frame::VideoFrame) -> Option<PackedFrame<'_>> {
    use scap::frame::VideoFrame;
    let (layout, width, height, data) = match frame {
        VideoFrame::BGRA(f) => (PixelLayout::BGRA, f.width, f.height, &f.data),
        VideoFrame::BGRx(f) => (PixelLayout::BGRA, f.width, f.height, &f.data),
        VideoFrame::RGBx(f) => (PixelLayout::RGBX, f.width, f.height, &f.data),
        VideoFrame::XBGR(f) => (PixelLayout::XBGR, f.width, f.height, &f.data),
        VideoFrame::RGB(f) => (PixelLayout::RGB, f.width, f.height, &f.data),
        _ => return None,
    };
    Some(PackedFrame {
        layout,
        width,
        height,
        data,
    })
}

impl FrameCapturer {
    /// Create a new frame capturer for the given source.
    ///
    /// `width` x `height` is the largest size frames are encoded at.
    pub const fn new(source_id: String, fps: u32, width: u32, height: u32) -> Self {
        Self {
            source_id,
            fps,
            width,
            height,
        }
    }

    /// Start capturing frames, sending them to the provided channel.
    ///
    /// Returns when `shutdown_rx` receives `true` or the capture source ends.
    /// This spawns a blocking thread since `scap::Capturer` is not async.
    pub fn start(
        self,
        frame_tx: mpsc::SyncSender<I420Frame>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<tokio::task::JoinHandle<()>, CaptureError> {
        let fps = self.fps;
        let width = self.width;
        let height = self.height;
        let source_id = self.source_id;

        let handle = tokio::task::spawn_blocking(move || {
            let Some(target) = find_target_by_id(&source_id) else {
                error!(source = %source_id, "Capture source is no longer available");
                return;
            };
            let options = build_capture_options(target, fps, width, height);

            let mut capturer = match scap::capturer::Capturer::build(options) {
                Ok(c) => c,
                Err(e) => {
                    error!(source = %source_id, error = %e, "Failed to create capturer");
                    return;
                }
            };

            capturer.start_capture();
            info!(source = %source_id, fps, "Capture started");

            let mut converter: Option<I420Converter> = None;
            let mut warned_format = false;

            loop {
                // Check shutdown signal (non-blocking)
                if *shutdown_rx.borrow() {
                    info!(source = %source_id, "Capture shutdown requested");
                    break;
                }

                match capturer.get_next_frame() {
                    Ok(scap::frame::Frame::Audio(_)) => {}
                    Ok(scap::frame::Frame::Video(video)) => {
                        let Some(frame) = packed_frame(&video) else {
                            if !warned_format {
                                warn!("Unsupported capture pixel format, skipping frames");
                                warned_format = true;
                            }
                            continue;
                        };

                        if converter
                            .as_ref()
                            .is_none_or(|c| c.layout() != frame.layout)
                        {
                            converter = Some(I420Converter::new(frame.layout, width, height));
                        }
                        let Some(conv) = converter.as_mut() else {
                            continue;
                        };

                        let (Ok(w), Ok(h)) =
                            (u32::try_from(frame.width), u32::try_from(frame.height))
                        else {
                            continue;
                        };
                        let Some(stride) = infer_row_stride(
                            frame.data.len(),
                            w,
                            h,
                            frame.layout.bytes_per_pixel(),
                        ) else {
                            // Empty (e.g. macOS idle frame) or truncated buffer.
                            continue;
                        };
                        let Some(i420) = conv.convert(frame.data, w, h, stride) else {
                            continue;
                        };

                        // Send frame, dropping if receiver is behind
                        if frame_tx.try_send(i420).is_err() {
                            debug!("Frame dropped (receiver behind)");
                        }
                    }
                    Err(e) => {
                        // End of stream (window closed, etc.)
                        info!(source = %source_id, error = %e, "Capture ended");
                        break;
                    }
                }
            }

            capturer.stop_capture();
            info!(source = %source_id, "Capture stopped");
        });

        Ok(handle)
    }
}
