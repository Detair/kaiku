//! Color Space Conversion
//!
//! Packed RGB-family pixels → I420 (YUV 4:2:0 planar) using BT.601 coefficients.
//!
//! Capture sources deliver frames of any size: a small game window, a 4K
//! monitor, or a webcam that picked a resolution other than the one asked for.
//! Rows may also be padded beyond `width * bytes_per_pixel`. The converter
//! uses each frame's actual width, height and row stride and box-scales it to
//! [`encode_dimensions`] (aspect ratio kept, never upscaled, even sizes).
//! Output buffers are reused across frames to avoid per-frame allocation.

use super::I420Frame;

/// Byte layout of a packed pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelLayout {
    bytes_per_pixel: usize,
    r: usize,
    g: usize,
    b: usize,
}

impl PixelLayout {
    /// `B, G, R, A` (also `B, G, R, x`).
    pub const BGRA: Self = Self::new(4, 2, 1, 0);
    /// `R, G, B, x`.
    pub const RGBX: Self = Self::new(4, 0, 1, 2);
    /// `x, B, G, R`.
    pub const XBGR: Self = Self::new(4, 3, 2, 1);
    /// `R, G, B` (webcam frames from `nokhwa`).
    pub const RGB: Self = Self::new(3, 0, 1, 2);

    const fn new(bytes_per_pixel: usize, r: usize, g: usize, b: usize) -> Self {
        Self {
            bytes_per_pixel,
            r,
            g,
            b,
        }
    }

    /// Bytes per pixel of this layout.
    pub const fn bytes_per_pixel(self) -> usize {
        self.bytes_per_pixel
    }
}

/// Size a `src_w` x `src_h` frame is encoded at for a tier of at most
/// `max_w` x `max_h`.
///
/// Keeps the aspect ratio, never upscales, and rounds down to even numbers
/// (I420 needs even sizes). The result is at least 2x2.
pub fn encode_dimensions(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let (src_w64, src_h64) = (u64::from(src_w.max(1)), u64::from(src_h.max(1)));
    let (max_w64, max_h64) = (u64::from(max_w.max(2)), u64::from(max_h.max(2)));

    let (w, h) = if src_w64 <= max_w64 && src_h64 <= max_h64 {
        (src_w64, src_h64)
    } else if src_w64 * max_h64 >= src_h64 * max_w64 {
        // Width is the limiting side.
        (max_w64, src_h64 * max_w64 / src_w64)
    } else {
        (src_w64 * max_h64 / src_h64, max_h64)
    };

    let even = |v: u64| ((v as u32) & !1).max(2);
    (even(w), even(h))
}

/// Row stride of a packed frame buffer of `len` bytes.
///
/// Capture backends either deliver tight rows or rows padded to a fixed
/// stride (`len == stride * height`). Returns `None` when the buffer is too
/// short to hold `width x height` pixels or the size is zero.
pub fn infer_row_stride(
    len: usize,
    width: u32,
    height: u32,
    bytes_per_pixel: usize,
) -> Option<usize> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let tight = w.checked_mul(bytes_per_pixel)?;
    if len < tight.checked_mul(h)? {
        return None;
    }
    if len.is_multiple_of(h) {
        Some(len / h)
    } else {
        Some(tight)
    }
}

/// Reusable packed-pixel → I420 converter with box downscaling.
pub struct I420Converter {
    layout: PixelLayout,
    max_width: u32,
    max_height: u32,
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    /// Source column range `[start, end)` per output column.
    col_ranges: Vec<(usize, usize)>,
    /// Source row range `[start, end)` per output row.
    row_ranges: Vec<(usize, usize)>,
    /// Key of the cached ranges: `(src_w, src_h, out_w, out_h)`.
    ranges_for: (u32, u32, u32, u32),
}

impl I420Converter {
    /// Create a converter for `layout` frames encoded at most at
    /// `max_width` x `max_height`.
    pub const fn new(layout: PixelLayout, max_width: u32, max_height: u32) -> Self {
        Self {
            layout,
            max_width,
            max_height,
            y: Vec::new(),
            u: Vec::new(),
            v: Vec::new(),
            col_ranges: Vec::new(),
            row_ranges: Vec::new(),
            ranges_for: (0, 0, 0, 0),
        }
    }

    /// Pixel layout this converter reads.
    pub const fn layout(&self) -> PixelLayout {
        self.layout
    }

    /// Convert one frame of `width` x `height` pixels whose rows start every
    /// `stride` bytes into an owned `I420Frame` of [`encode_dimensions`].
    ///
    /// Returns `None` (frame skipped) when the size is zero, the stride is
    /// shorter than a row, or the buffer does not hold every row.
    pub fn convert(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
        stride: usize,
    ) -> Option<I420Frame> {
        let bpp = self.layout.bytes_per_pixel;
        let (src_w, src_h) = (width as usize, height as usize);
        if src_w == 0 || src_h == 0 || stride < src_w.checked_mul(bpp)? {
            return None;
        }
        // Last row only needs its pixels, not its padding.
        let needed = stride.checked_mul(src_h - 1)?.checked_add(src_w * bpp)?;
        if data.len() < needed {
            return None;
        }

        let (out_w, out_h) = encode_dimensions(width, height, self.max_width, self.max_height);
        self.prepare(width, height, out_w, out_h);

        let (ow, oh) = (out_w as usize, out_h as usize);
        let uv_w = ow / 2;
        let layout = self.layout;

        for orow in (0..oh).step_by(2) {
            for ocol in (0..ow).step_by(2) {
                let mut sum_u: i32 = 0;
                let mut sum_v: i32 = 0;

                for dy in 0..2 {
                    let rows = self.row_ranges[orow + dy];
                    for dx in 0..2 {
                        let cols = self.col_ranges[ocol + dx];
                        let (r, g, b) = box_average(data, layout, stride, rows, cols);

                        let y_val = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
                        self.y[(orow + dy) * ow + ocol + dx] = y_val.clamp(0, 255) as u8;

                        sum_u += (-38 * r - 74 * g + 112 * b + 128) >> 8;
                        sum_v += (112 * r - 94 * g - 18 * b + 128) >> 8;
                    }
                }

                let uv_idx = (orow / 2) * uv_w + ocol / 2;
                self.u[uv_idx] = ((sum_u / 4) + 128).clamp(0, 255) as u8;
                self.v[uv_idx] = ((sum_v / 4) + 128).clamp(0, 255) as u8;
            }
        }

        Some(I420Frame {
            y: self.y.clone(),
            u: self.u.clone(),
            v: self.v.clone(),
            width: out_w,
            height: out_h,
        })
    }

    /// Size the planes and source ranges for this input/output size.
    fn prepare(&mut self, src_w: u32, src_h: u32, out_w: u32, out_h: u32) {
        let key = (src_w, src_h, out_w, out_h);
        if self.ranges_for == key {
            return;
        }
        let (ow, oh) = (out_w as usize, out_h as usize);
        self.y.resize(ow * oh, 0);
        self.u.resize((ow / 2) * (oh / 2), 0);
        self.v.resize((ow / 2) * (oh / 2), 0);
        self.col_ranges = source_ranges(src_w as usize, ow);
        self.row_ranges = source_ranges(src_h as usize, oh);
        self.ranges_for = key;
    }
}

/// For each of `out` output samples, the `[start, end)` source range it
/// averages. Every range is non-empty and inside `0..src`.
fn source_ranges(src: usize, out: usize) -> Vec<(usize, usize)> {
    (0..out)
        .map(|i| {
            let start = (i * src / out).min(src - 1);
            let end = ((i + 1) * src / out).clamp(start + 1, src);
            (start, end)
        })
        .collect()
}

/// Average `(r, g, b)` over a source box.
fn box_average(
    data: &[u8],
    layout: PixelLayout,
    stride: usize,
    rows: (usize, usize),
    cols: (usize, usize),
) -> (i32, i32, i32) {
    let bpp = layout.bytes_per_pixel;
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for row in rows.0..rows.1 {
        let line = &data[row * stride + cols.0 * bpp..row * stride + cols.1 * bpp];
        for px in line.chunks_exact(bpp) {
            r += u32::from(px[layout.r]);
            g += u32::from(px[layout.g]);
            b += u32::from(px[layout.b]);
        }
    }
    let count = ((rows.1 - rows.0) * (cols.1 - cols.0)) as u32;
    ((r / count) as i32, (g / count) as i32, (b / count) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_tight(layout: PixelLayout, data: &[u8], width: u32, height: u32) -> I420Frame {
        let mut converter = I420Converter::new(layout, 1920, 1080);
        converter
            .convert(
                data,
                width,
                height,
                width as usize * layout.bytes_per_pixel(),
            )
            .expect("valid frame")
    }

    #[test]
    fn test_convert_black_frame() {
        let width = 4u32;
        let height = 4u32;
        // Black BGRA: all zeros (alpha = 255)
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        for i in (0..bgra.len()).step_by(4) {
            bgra[i + 3] = 255; // alpha
        }

        let frame = convert_tight(PixelLayout::BGRA, &bgra, width, height);

        assert_eq!(frame.width, width);
        assert_eq!(frame.height, height);
        assert_eq!(frame.y.len(), (width * height) as usize);
        assert_eq!(frame.u.len(), ((width / 2) * (height / 2)) as usize);
        assert_eq!(frame.v.len(), ((width / 2) * (height / 2)) as usize);

        // Black should produce Y=16, U=128, V=128 in BT.601
        for &y in &frame.y {
            assert_eq!(y, 16, "Y should be 16 for black");
        }
        for &u in &frame.u {
            assert_eq!(u, 128, "U should be 128 for black");
        }
        for &v in &frame.v {
            assert_eq!(v, 128, "V should be 128 for black");
        }
    }

    #[test]
    fn test_convert_white_frame() {
        let width = 4u32;
        let height = 4u32;
        // White BGRA: B=255, G=255, R=255, A=255
        let bgra = vec![255u8; (width * height * 4) as usize];

        let frame = convert_tight(PixelLayout::BGRA, &bgra, width, height);

        // White should produce Y=235 in BT.601
        for &y in &frame.y {
            assert_eq!(y, 235, "Y should be 235 for white");
        }
    }

    #[test]
    fn test_rgb_convert_black_frame() {
        let width = 4u32;
        let height = 4u32;
        // Black RGB: all zeros
        let rgb = vec![0u8; (width * height * 3) as usize];

        let frame = convert_tight(PixelLayout::RGB, &rgb, width, height);

        assert_eq!(frame.width, width);
        assert_eq!(frame.height, height);
        assert_eq!(frame.y.len(), (width * height) as usize);
        assert_eq!(frame.u.len(), ((width / 2) * (height / 2)) as usize);
        assert_eq!(frame.v.len(), ((width / 2) * (height / 2)) as usize);

        for &y in &frame.y {
            assert_eq!(y, 16, "Y should be 16 for black");
        }
        for &u in &frame.u {
            assert_eq!(u, 128, "U should be 128 for black");
        }
        for &v in &frame.v {
            assert_eq!(v, 128, "V should be 128 for black");
        }
    }

    #[test]
    fn test_rgb_convert_white_frame() {
        let width = 4u32;
        let height = 4u32;
        // White RGB: R=255, G=255, B=255
        let rgb = vec![255u8; (width * height * 3) as usize];

        let frame = convert_tight(PixelLayout::RGB, &rgb, width, height);

        for &y in &frame.y {
            assert_eq!(y, 235, "Y should be 235 for white");
        }
    }

    #[test]
    fn channel_order_follows_layout() {
        // Pure red in each layout must give the same Y (BT.601: 82).
        let cases: [(PixelLayout, [u8; 4]); 3] = [
            (PixelLayout::BGRA, [0, 0, 255, 255]),
            (PixelLayout::RGBX, [255, 0, 0, 0]),
            (PixelLayout::XBGR, [0, 0, 0, 255]),
        ];
        for (layout, px) in cases {
            let data: Vec<u8> = px.iter().copied().cycle().take(2 * 2 * 4).collect();
            let frame = convert_tight(layout, &data, 2, 2);
            assert!(
                frame.y.iter().all(|&y| y == 82),
                "{layout:?}: {:?}",
                frame.y
            );
        }
    }

    #[test]
    fn small_window_is_not_upscaled_and_even() {
        // Smaller than the tier: kept as is, rounded down to even.
        assert_eq!(encode_dimensions(801, 601, 1920, 1080), (800, 600));
        assert_eq!(encode_dimensions(640, 360, 1920, 1080), (640, 360));
        // Larger than the tier: scaled down with the aspect ratio kept.
        assert_eq!(encode_dimensions(3840, 2160, 1920, 1080), (1920, 1080));
        assert_eq!(encode_dimensions(2560, 1080, 1920, 1080), (1920, 810));
        assert_eq!(encode_dimensions(1000, 3000, 1920, 1080), (360, 1080));
        // Degenerate sizes never produce an odd or zero dimension.
        assert_eq!(encode_dimensions(1, 1, 1920, 1080), (2, 2));
        let (w, h) = encode_dimensions(1366, 769, 1280, 720);
        assert!(w <= 1280 && h <= 720 && w % 2 == 0 && h % 2 == 0);
    }

    #[test]
    fn convert_respects_row_stride() {
        // 4x2 BGRA frame with rows padded to 24 bytes (16 used + 8 padding).
        // Visible pixels are white; the padding is black and must be ignored.
        let width = 4u32;
        let height = 2u32;
        let stride = 24usize;
        let mut data = vec![0u8; stride * height as usize];
        for row in 0..height as usize {
            for byte in 0..(width as usize * 4) {
                data[row * stride + byte] = 255;
            }
        }

        let mut converter = I420Converter::new(PixelLayout::BGRA, 1920, 1080);
        let frame = converter
            .convert(&data, width, height, stride)
            .expect("valid frame");

        assert_eq!((frame.width, frame.height), (4, 2));
        assert!(frame.y.iter().all(|&y| y == 235), "padding leaked into Y");
        assert!(frame.u.iter().all(|&u| u == 128));
    }

    #[test]
    fn convert_handles_frame_smaller_than_tier() {
        // A 301x157 game window on a 1080p tier: no panic, even output size,
        // planes sized for the output.
        let (width, height) = (301u32, 157u32);
        let data = vec![0u8; (width * height * 4) as usize];

        let mut converter = I420Converter::new(PixelLayout::BGRA, 1920, 1080);
        let frame = converter
            .convert(&data, width, height, width as usize * 4)
            .expect("valid frame");

        assert_eq!((frame.width, frame.height), (300, 156));
        assert_eq!(frame.y.len(), 300 * 156);
        assert_eq!(frame.u.len(), 150 * 78);
        assert_eq!(frame.v.len(), 150 * 78);

        // A larger frame on the same converter is scaled down to the tier.
        let big = vec![255u8; 3840 * 2160 * 4];
        let frame = converter
            .convert(&big, 3840, 2160, 3840 * 4)
            .expect("valid frame");
        assert_eq!((frame.width, frame.height), (1920, 1080));
        assert_eq!(frame.y.len(), 1920 * 1080);
        assert!(frame.y.iter().all(|&y| y == 235));

        // Empty or truncated frames are rejected instead of panicking.
        assert!(converter.convert(&[], 0, 0, 0).is_none());
        assert!(converter
            .convert(&data[..100], width, height, width as usize * 4)
            .is_none());
    }

    #[test]
    fn infer_row_stride_handles_padding_and_short_buffers() {
        assert_eq!(infer_row_stride(16 * 2, 4, 2, 4), Some(16));
        assert_eq!(infer_row_stride(24 * 2, 4, 2, 4), Some(24));
        // Trailing bytes that are not a whole number of rows: tight rows.
        assert_eq!(infer_row_stride(16 * 2 + 3, 4, 2, 4), Some(16));
        assert_eq!(infer_row_stride(10, 4, 2, 4), None);
        assert_eq!(infer_row_stride(0, 0, 0, 4), None);
    }
}
