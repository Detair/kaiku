//! Frames of a `ScreenCast` portal session, read from `PipeWire` (Linux).
//!
//! Uses the safe `pipewire` API only (the workspace forbids `unsafe`). The
//! capture thread converts each buffer to I420 and `try_send`s it (dropped
//! when the encoder is behind). It ends on shutdown or when the stream goes
//! away (the user stopped sharing in the system's indicator, the window
//! closed) or the encoder is gone: then its reason is stored, the frame
//! sender is dropped and the publication reports the end to the frontend.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use pipewire as pw;
use pw::spa;
use pw::spa::buffer::{ChunkFlags, DataType};
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::param::ParamType;
use pw::spa::pod::Pod;
use pw::spa::utils::{Direction, SpaTypes};
use pw::stream::{StreamFlags, StreamState};
use tokio::sync::watch;
use tracing::{info, warn};

use super::convert::{infer_row_stride, I420Converter, PixelLayout};
use super::portal::PortalStream;
use super::I420Frame;
use crate::video::publication::{end_reason, offer_frame, run_capture, EndReason};
use crate::video::QualityParams;

/// Our converter's layout for a negotiated `PipeWire` format.
pub const fn layout_for(format: VideoFormat) -> Option<PixelLayout> {
    match format {
        VideoFormat::BGRx | VideoFormat::BGRA => Some(PixelLayout::BGRA),
        VideoFormat::RGBx | VideoFormat::RGBA => Some(PixelLayout::RGBX),
        VideoFormat::xBGR | VideoFormat::ABGR => Some(PixelLayout::XBGR),
        VideoFormat::RGB => Some(PixelLayout::RGB),
        _ => None,
    }
}

/// The pixel bytes of one buffer: `chunk_offset`/`chunk_size` as `PipeWire`
/// reports them, clamped to the mapped data.
pub fn chunk_bytes(data: &[u8], chunk_offset: u32, chunk_size: u32) -> &[u8] {
    let start = (chunk_offset as usize).min(data.len());
    // Size 0 is an empty buffer (`usable_chunk` skips it before this).
    let end = start.saturating_add(chunk_size as usize).min(data.len());
    &data[start..end]
}

/// Row stride: `PipeWire`'s when it gives one, else inferred from the size.
pub fn row_stride(
    bytes: &[u8],
    width: u32,
    height: u32,
    layout: PixelLayout,
    stride: i32,
) -> Option<usize> {
    match usize::try_from(stride) {
        Ok(s) if s >= width as usize * layout.bytes_per_pixel() => Some(s),
        _ => infer_row_stride(bytes.len(), width, height, layout.bytes_per_pixel()),
    }
}

struct Negotiated {
    info: VideoInfoRaw,
    layout: Option<PixelLayout>,
}

fn format_params(fps: u32) -> Result<Vec<u8>, String> {
    let obj = spa::pod::object!(
        SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        spa::pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::RGBx,
            VideoFormat::BGRA,
            VideoFormat::RGBA,
            VideoFormat::xBGR,
            VideoFormat::RGB,
        ),
        spa::pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            FormatProperties::VideoMaxFramerate,
            Fraction,
            spa::utils::Fraction { num: fps, denom: 1 }
        ),
    );
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|e| e.to_string())
}

/// No usable picture this long after the stream was connected: the share
/// ends with a clear reason instead of staying black.
pub const NO_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// Paused this long after it was streaming: the system stopped the share
/// (its "stop sharing" control pauses or unlinks the node).
pub const PAUSED_END: Duration = Duration::from_secs(2);

/// Decides when a portal stream is over.
#[derive(Debug)]
pub struct StreamWatchdog {
    connected_at: Instant,
    got_frame: bool,
    streamed: bool,
    paused_since: Option<Instant>,
}

impl StreamWatchdog {
    pub const fn new(connected_at: Instant) -> Self {
        Self {
            connected_at,
            got_frame: false,
            streamed: false,
            paused_since: None,
        }
    }

    /// A frame was converted and handed on.
    pub const fn frame(&mut self) {
        self.got_frame = true;
    }

    /// The stream's current state.
    pub fn state(&mut self, state: &StreamState, now: Instant) {
        match state {
            StreamState::Streaming => {
                self.streamed = true;
                self.paused_since = None;
            }
            StreamState::Paused if self.streamed => {
                self.paused_since.get_or_insert(now);
            }
            _ => {}
        }
    }

    /// `Some(reason)` when the capture should end now.
    pub fn check(&self, now: Instant, session_closed: bool) -> Option<&'static str> {
        if session_closed {
            return Some(end_reason::ENDED);
        }
        if self
            .paused_since
            .is_some_and(|at| now.duration_since(at) >= PAUSED_END)
        {
            return Some(end_reason::ENDED);
        }
        if !self.got_frame && now.duration_since(self.connected_at) >= NO_FRAME_TIMEOUT {
            return Some(end_reason::NO_FRAMES);
        }
        None
    }
}

/// Whether a buffer can be read as packed pixels: mapped memory (not a
/// DMA-BUF, which may be tiled), a non-empty chunk, not marked corrupted.
pub fn usable_chunk(data_type: DataType, corrupted: bool, size: u32) -> bool {
    (data_type == DataType::MemFd || data_type == DataType::MemPtr) && !corrupted && size > 0
}

/// Read the portal stream until shutdown or until it ends.
fn run(
    stream: PortalStream,
    params: QualityParams,
    frames: std_mpsc::SyncSender<I420Frame>,
    shutdown: &watch::Receiver<bool>,
) -> Result<Option<&'static str>, String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::Context::new(&mainloop).map_err(|e| e.to_string())?;
    // `_session` keeps the portal session open until this function returns.
    let PortalStream {
        fd,
        node_id,
        closed,
        guard: _session,
        ..
    } = stream;
    let core = context.connect_fd(fd, None).map_err(|e| e.to_string())?;
    let pw_stream = pw::stream::Stream::new(
        &core,
        "kaiku-screen",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| e.to_string())?;

    let ended = Rc::new(Cell::new(false));
    let ended_cb = ended.clone();
    let delivered = Rc::new(Cell::new(false));
    let delivered_cb = delivered.clone();
    let sink_gone = Rc::new(Cell::new(false));
    let sink_gone_cb = sink_gone.clone();
    let mut converter: Option<I420Converter> = None;
    let mut warned_type = false;
    let max = (params.width, params.height);

    let _listener = pw_stream
        .add_local_listener_with_user_data(Negotiated {
            info: VideoInfoRaw::default(),
            layout: None,
        })
        .state_changed(move |_, _, _old, new| match new {
            StreamState::Error(e) => {
                warn!("Screen cast stream error: {e}");
                ended_cb.set(true);
            }
            StreamState::Unconnected => ended_cb.set(true),
            _ => {}
        })
        .param_changed(|_, data, id, param| {
            let Some(param) = param else { return };
            if id != ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type != MediaType::Video || media_subtype != MediaSubtype::Raw {
                return;
            }
            if data.info.parse(param).is_ok() {
                data.layout = layout_for(data.info.format());
                info!(format = ?data.info.format(), size = ?data.info.size(), "Screen cast format");
            }
        })
        .process(move |stream, data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(layout) = data.layout else { return };
            let size = data.info.size();
            let datas = buffer.datas_mut();
            let Some(first) = datas.first_mut() else {
                return;
            };
            let data_type = first.type_();
            let (offset, chunk_size, stride, corrupted) = {
                let chunk = first.chunk();
                (
                    chunk.offset(),
                    chunk.size(),
                    chunk.stride(),
                    chunk.flags().contains(ChunkFlags::CORRUPTED),
                )
            };
            if !usable_chunk(data_type, corrupted, chunk_size) {
                if !warned_type && data_type != DataType::MemFd && data_type != DataType::MemPtr {
                    warn!(?data_type, "Screen cast buffers are not mapped memory");
                    warned_type = true;
                }
                return;
            }
            let Some(mapped) = first.data() else { return };
            let bytes = chunk_bytes(mapped, offset, chunk_size);
            let Some(stride) = row_stride(bytes, size.width, size.height, layout, stride) else {
                return;
            };
            let conv = converter.get_or_insert_with(|| I420Converter::new(layout, max.0, max.1));
            if conv.layout() != layout {
                *conv = I420Converter::new(layout, max.0, max.1);
            }
            if let Some(i420) = conv.convert(bytes, size.width, size.height, stride) {
                delivered_cb.set(true);
                if !offer_frame(&frames, i420) {
                    sink_gone_cb.set(true);
                }
            }
        })
        .register()
        .map_err(|e| e.to_string())?;

    let format = format_params(params.fps)?;
    let pod = Pod::from_bytes(&format).ok_or("format pod")?;
    pw_stream
        .connect(
            Direction::Input,
            Some(node_id),
            // Never relinked to another node once the portal's node is gone.
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS | StreamFlags::DONT_RECONNECT,
            &mut [pod],
        )
        .map_err(|e| e.to_string())?;

    let pw_loop = mainloop.loop_();
    let mut watchdog = StreamWatchdog::new(Instant::now());
    let reason = loop {
        if *shutdown.borrow() {
            break None;
        }
        pw_loop.iterate(Duration::from_millis(50));
        let now = Instant::now();
        if delivered.get() {
            watchdog.frame();
        }
        watchdog.state(&pw_stream.state(), now);
        if ended.get() {
            info!("Screen cast stream ended");
            break Some(end_reason::ENDED);
        }
        if sink_gone.get() {
            // The encoder ended (and said why): nothing takes frames.
            info!("Screen cast capture ends: the encoder is gone");
            break None;
        }
        if let Some(reason) = watchdog.check(now, closed.load(Ordering::Acquire)) {
            info!(reason, "Screen cast capture ends");
            break Some(reason);
        }
    };
    let _ = pw_stream.disconnect();
    Ok(reason)
}

/// Start the capture thread for a portal stream. Its end reason (no
/// picture, stream over) is written to `reason` before the last frame
/// sender is dropped ([`run_capture`]); the `PipeWire` teardown inside
/// `run` happens after its own sender is gone.
pub fn start(
    stream: PortalStream,
    params: QualityParams,
    frames: std_mpsc::SyncSender<I420Frame>,
    shutdown: watch::Receiver<bool>,
    reason: EndReason,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        run_capture(frames, &reason, |frames| {
            match run(stream, params, frames, &shutdown) {
                Ok(why) => why,
                Err(e) => {
                    warn!("Screen cast capture failed: {e}");
                    Some(end_reason::NO_FRAMES)
                }
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiated_formats_map_to_converter_layouts() {
        assert_eq!(
            layout_for(VideoFormat::BGRx).map(|l| l.bytes_per_pixel()),
            Some(4)
        );
        assert!(layout_for(VideoFormat::RGB).is_some());
        assert!(
            layout_for(VideoFormat::NV12).is_none(),
            "planar formats are not offered"
        );
    }

    #[test]
    fn chunk_is_clamped_to_the_mapped_buffer() {
        let data = [0u8; 100];
        assert_eq!(chunk_bytes(&data, 10, 20).len(), 20);
        assert_eq!(chunk_bytes(&data, 90, 50).len(), 10);
        assert_eq!(chunk_bytes(&data, 200, 10).len(), 0);
        assert_eq!(chunk_bytes(&data, 0, 0).len(), 0, "size 0 is empty");
    }

    #[test]
    fn stride_from_pipewire_or_inferred() {
        let bytes = vec![0u8; 8 * 2 * 4];
        let bgra = PixelLayout::BGRA;
        assert_eq!(row_stride(&bytes, 8, 2, bgra, 32), Some(32));
        // A stride shorter than a row is ignored.
        assert_eq!(row_stride(&bytes, 8, 2, bgra, 4), Some(32));
        assert_eq!(row_stride(&bytes, 8, 2, bgra, -1), Some(32));
    }

    #[test]
    fn format_params_serialize() {
        let bytes = format_params(30).expect("pod");
        assert!(Pod::from_bytes(&bytes).is_some());
    }

    #[test]
    fn only_mapped_non_empty_uncorrupted_chunks_are_read() {
        assert!(usable_chunk(DataType::MemFd, false, 100));
        assert!(usable_chunk(DataType::MemPtr, false, 100));
        assert!(
            !usable_chunk(DataType::DmaBuf, false, 100),
            "DMA-BUF may be tiled"
        );
        assert!(!usable_chunk(DataType::MemFd, true, 100), "corrupted");
        assert!(!usable_chunk(DataType::MemFd, false, 0), "empty buffer");
    }

    #[test]
    fn no_picture_within_the_timeout_ends_with_no_frames() {
        let t0 = Instant::now();
        let mut w = StreamWatchdog::new(t0);
        w.state(&StreamState::Streaming, t0);
        assert_eq!(w.check(t0 + Duration::from_secs(4), false), None);
        assert_eq!(
            w.check(t0 + NO_FRAME_TIMEOUT, false),
            Some(end_reason::NO_FRAMES)
        );
        // With a picture it keeps going.
        let mut w = StreamWatchdog::new(t0);
        w.frame();
        assert_eq!(w.check(t0 + Duration::from_mins(1), false), None);
    }

    #[test]
    fn stop_in_the_system_indicator_ends_the_capture() {
        let t0 = Instant::now();
        // The portal closes the session.
        let mut w = StreamWatchdog::new(t0);
        w.frame();
        assert_eq!(w.check(t0, true), Some(end_reason::ENDED));
        // Or the stream falls back to paused for good.
        w.state(&StreamState::Streaming, t0);
        w.state(&StreamState::Paused, t0 + Duration::from_secs(1));
        assert_eq!(w.check(t0 + Duration::from_secs(2), false), None);
        assert_eq!(
            w.check(t0 + Duration::from_secs(1) + PAUSED_END, false),
            Some(end_reason::ENDED)
        );
        // A short pause that resumes does not end it.
        w.state(&StreamState::Streaming, t0 + Duration::from_secs(10));
        assert_eq!(w.check(t0 + Duration::from_secs(20), false), None);
        // Paused before it ever streamed (negotiating) is not an end.
        let mut fresh = StreamWatchdog::new(t0);
        fresh.frame();
        fresh.state(&StreamState::Paused, t0);
        assert_eq!(fresh.check(t0 + Duration::from_secs(9), false), None);
    }
}
