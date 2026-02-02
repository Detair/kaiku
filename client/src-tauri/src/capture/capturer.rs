//! Frame Capturer
//!
//! Wraps `scap::Capturer` to produce I420 frames at the target FPS.
//! Runs on a background thread and sends frames via `mpsc::Sender`.

use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};

use super::convert::BgraToI420Converter;
use super::source::build_capture_options;
use super::{CaptureError, I420Frame};

/// Frame capturer that produces I420 frames from a native capture source.
pub struct FrameCapturer {
    target: scap::Target,
    source_id: String,
    fps: u32,
    width: u32,
    height: u32,
}

impl FrameCapturer {
    /// Create a new frame capturer for the given source.
    ///
    /// `target` must be a previously resolved `scap::Target`.
    /// `source_id` is used for logging only.
    pub const fn new(
        target: scap::Target,
        source_id: String,
        fps: u32,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            target,
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
        frame_tx: mpsc::Sender<I420Frame>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Result<tokio::task::JoinHandle<()>, CaptureError> {
        let target = self.target;
        let fps = self.fps;
        let width = self.width;
        let height = self.height;
        let source_id = self.source_id;

        let handle = tokio::task::spawn_blocking(move || {
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

            let mut converter = BgraToI420Converter::new(width, height);

            loop {
                // Check shutdown signal (non-blocking)
                if *shutdown_rx.borrow() {
                    info!(source = %source_id, "Capture shutdown requested");
                    break;
                }

                match capturer.get_next_frame() {
                    Ok(frame) => {
                        let bgra_data = if let scap::frame::Frame::BGRA(bgra) = frame {
                            bgra.data
                        } else {
                            warn!("Unexpected frame format, skipping");
                            continue;
                        };

                        let i420 = converter.convert_owned(&bgra_data);

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
