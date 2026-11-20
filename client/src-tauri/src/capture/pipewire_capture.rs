//! Frames of a ScreenCast portal session, read from PipeWire (Linux).
//!
//! Uses the safe `pipewire` API only (the workspace forbids `unsafe`). The
//! capture thread converts each buffer to I420 and `try_send`s it (dropped
//! when the encoder is behind). It ends on shutdown or when the stream goes
//! away (the user stopped sharing in the system's indicator, the window
//! closed): then the frame sender is dropped and the publication reports
//! the end to the frontend.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use pipewire as pw;
use pw::spa;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::param::ParamType;
use pw::spa::pod::Pod;
use pw::spa::utils::{Direction, SpaTypes};
use pw::stream::{StreamFlags, StreamState};
use tokio::sync::watch;
use tracing::{debug, info, warn};

use super::convert::{infer_row_stride, I420Converter, PixelLayout};
use super::portal::PortalStream;
use super::I420Frame;
use crate::video::QualityParams;

/// Our converter's layout for a negotiated PipeWire format.
pub fn layout_for(format: VideoFormat) -> Option<PixelLayout> {
    match format {
        VideoFormat::BGRx | VideoFormat::BGRA => Some(PixelLayout::BGRA),
        VideoFormat::RGBx | VideoFormat::RGBA => Some(PixelLayout::RGBX),
        VideoFormat::xBGR | VideoFormat::ABGR => Some(PixelLayout::XBGR),
        VideoFormat::RGB => Some(PixelLayout::RGB),
        _ => None,
    }
}

/// The pixel bytes of one buffer: `chunk_offset`/`chunk_size` as PipeWire
/// reports them, clamped to the mapped data.
pub fn chunk_bytes(data: &[u8], chunk_offset: u32, chunk_size: u32) -> &[u8] {
    let start = (chunk_offset as usize).min(data.len());
    let end = if chunk_size == 0 {
        data.len()
    } else {
        start.saturating_add(chunk_size as usize).min(data.len())
    };
    &data[start..end]
}

/// Row stride: PipeWire's when it gives one, else inferred from the size.
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

/// Read the portal stream until shutdown or until it ends.
fn run(
    stream: PortalStream,
    params: QualityParams,
    frames: std_mpsc::SyncSender<I420Frame>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), String> {
    pw::init();
    let mainloop = pw::main_loop::MainLoop::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::Context::new(&mainloop).map_err(|e| e.to_string())?;
    let PortalStream { fd, node_id, .. } = stream;
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
    let mut converter: Option<I420Converter> = None;
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
            let (offset, chunk_size, stride) = {
                let chunk = first.chunk();
                (chunk.offset(), chunk.size(), chunk.stride())
            };
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
                if frames.try_send(i420).is_err() {
                    debug!("Screen frame dropped (encoder behind)");
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
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut [pod],
        )
        .map_err(|e| e.to_string())?;

    let pw_loop = mainloop.loop_();
    let mut streaming = false;
    loop {
        if *shutdown.borrow() {
            break;
        }
        pw_loop.iterate(Duration::from_millis(50));
        match pw_stream.state() {
            StreamState::Streaming => streaming = true,
            StreamState::Unconnected if streaming => ended.set(true),
            _ => {}
        }
        if ended.get() {
            info!("Screen cast stream ended");
            break;
        }
    }
    let _ = pw_stream.disconnect();
    Ok(())
}

/// Start the capture thread for a portal stream.
pub fn start(
    stream: PortalStream,
    params: QualityParams,
    frames: std_mpsc::SyncSender<I420Frame>,
    shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        if let Err(e) = run(stream, params, frames, shutdown) {
            warn!("Screen cast capture failed: {e}");
        }
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
        assert_eq!(chunk_bytes(&data, 0, 0).len(), 100, "size 0: whole buffer");
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
}
