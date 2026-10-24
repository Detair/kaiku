//! Color Space Conversion
//!
//! Packed RGB-family pixels → I420 (YUV 4:2:0 planar) using BT.601 coefficients.
//!
//! Capture sources deliver frames of any size: a small game window, a 4K
//! monitor, or a webcam that picked a resolution other than the one asked for.
//! Rows may also be padded beyond `width * bytes_per_pixel`. The converter
//! uses each frame's actual width, height and row stride and box-scales it to
//! [`encode_dimensions`] (aspect ratio kept, never upscaled, even sizes).
//! Each frame gets freshly zeroed planes that are moved into the result;
//! frames that need no scaling skip the box filter.

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

/// Largest row alignment (in bytes) accepted when inferring a padded stride.
const MAX_ROW_ALIGNMENT: usize = 4096;

/// Row stride of a packed frame buffer of `len` bytes.
///
/// Capture backends deliver either tight rows or rows padded up to an
/// alignment boundary (Windows: `len == stride * height`). `len / height` is
/// only taken as the stride when it equals the tight row size rounded up to
/// a power-of-two alignment (at most [`MAX_ROW_ALIGNMENT`]); any other
/// buffer size, such as `PipeWire`'s `maxsize` which may exceed
/// `stride * height`, is read with tight rows. Returns `None` when the buffer
/// is too short to hold `width x height` pixels or the size is zero.
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
    if !len.is_multiple_of(h) {
        return Some(tight);
    }
    let candidate = len / h;
    let aligned = (1..=MAX_ROW_ALIGNMENT.trailing_zeros())
        .map(|shift| 1usize << shift)
        .any(|align| tight.next_multiple_of(align) == candidate);
    Some(if aligned { candidate } else { tight })
}

/// Reusable packed-pixel → I420 converter with box downscaling.
pub struct I420Converter {
    layout: PixelLayout,
    max_width: u32,
    max_height: u32,
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
        let (ow, oh) = (out_w as usize, out_h as usize);
        let layout = self.layout;

        // Fresh zeroed planes (calloc, no copy) are moved into the frame
        // instead of cloning reused buffers.
        let mut y = vec![0u8; ow * oh];
        let mut u = vec![0u8; (ow / 2) * (oh / 2)];
        let mut v = vec![0u8; (ow / 2) * (oh / 2)];

        let near = |src: usize, out: usize| src.checked_sub(out).is_some_and(|d| d <= 1);
        if near(src_w, ow) && near(src_h, oh) {
            // 1:1 (an odd last column/row is cropped): read pixels directly.
            fill_planes(ow, oh, &mut y, &mut u, &mut v, |row, col| {
                let px = &data[row * stride + col * bpp..];
                (
                    i32::from(px[layout.r]),
                    i32::from(px[layout.g]),
                    i32::from(px[layout.b]),
                )
            });
        } else {
            self.prepare_ranges(width, height, out_w, out_h);
            let (rows, cols) = (&self.row_ranges, &self.col_ranges);
            fill_planes(ow, oh, &mut y, &mut u, &mut v, |row, col| {
                box_average(data, layout, stride, rows[row], cols[col])
            });
        }

        Some(I420Frame {
            y,
            u,
            v,
            width: out_w,
            height: out_h,
        })
    }

    /// Cache the source box ranges for this input/output size.
    fn prepare_ranges(&mut self, src_w: u32, src_h: u32, out_w: u32, out_h: u32) {
        let key = (src_w, src_h, out_w, out_h);
        if self.ranges_for == key {
            return;
        }
        self.col_ranges = source_ranges(src_w as usize, out_w as usize);
        self.row_ranges = source_ranges(src_h as usize, out_h as usize);
        self.ranges_for = key;
    }
}

/// Fill I420 planes of `ow` x `oh` (both even) from an `(r, g, b)` sampler
/// indexed by output `(row, col)`, using BT.601 coefficients.
#[allow(clippy::many_single_char_names)]
fn fill_planes(
    ow: usize,
    oh: usize,
    y: &mut [u8],
    u: &mut [u8],
    v: &mut [u8],
    sample: impl Fn(usize, usize) -> (i32, i32, i32),
) {
    let uv_w = ow / 2;
    for orow in (0..oh).step_by(2) {
        for ocol in (0..ow).step_by(2) {
            let mut sum_u: i32 = 0;
            let mut sum_v: i32 = 0;

            for dy in 0..2 {
                for dx in 0..2 {
                    let (r, g, b) = sample(orow + dy, ocol + dx);

                    let y_val = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
                    y[(orow + dy) * ow + ocol + dx] = y_val.clamp(0, 255) as u8;

                    sum_u += (-38 * r - 74 * g + 112 * b + 128) >> 8;
                    sum_v += (112 * r - 94 * g - 18 * b + 128) >> 8;
                }
            }

            let uv_idx = (orow / 2) * uv_w + ocol / 2;
            u[uv_idx] = ((sum_u / 4) + 128).clamp(0, 255) as u8;
            v[uv_idx] = ((sum_v / 4) + 128).clamp(0, 255) as u8;
        }
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
    let (mut r, mut g, mut b) = (0u64, 0u64, 0u64);
    for row in rows.0..rows.1 {
        let line = &data[row * stride + cols.0 * bpp..row * stride + cols.1 * bpp];
        for px in line.chunks_exact(bpp) {
            r += u64::from(px[layout.r]);
            g += u64::from(px[layout.g]);
            b += u64::from(px[layout.b]);
        }
    }
    let count = ((rows.1 - rows.0) * (cols.1 - cols.0)) as u64;
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
        // Rows padded to an alignment boundary (301 px BGRA → 1204 → 1280).
        assert_eq!(infer_row_stride(1280 * 157, 301, 157, 4), Some(1280));
        // Trailing bytes that are not a whole number of rows: tight rows.
        assert_eq!(infer_row_stride(16 * 2 + 3, 4, 2, 4), Some(16));
        assert_eq!(infer_row_stride(10, 4, 2, 4), None);
        assert_eq!(infer_row_stride(0, 0, 0, 4), None);
    }

    #[test]
    fn oversized_buffer_divisible_by_height_uses_tight_stride() {
        // PipeWire hands over `maxsize`, which can exceed stride * height.
        // When the excess happens to divide by the height, len / height is
        // not an alignment of the row and must not be taken as the stride.
        assert_eq!(
            infer_row_stride(1204 * 157 + 157 * 100, 301, 157, 4),
            Some(1204)
        );
        assert_eq!(infer_row_stride(16 * 2 + 2 * 100, 4, 2, 4), Some(16));
        // Hugely oversized buffer (several frames' worth) → tight rows too.
        assert_eq!(
            infer_row_stride(1920 * 4 * 1080 * 3, 1920, 1080, 4),
            Some(1920 * 4)
        );
    }

    #[test]
    fn padded_odd_frame_is_downscaled_without_reading_padding() {
        // 641x481 white BGRA frame, rows padded to 2816 bytes with black,
        // scaled to fit 320x240.
        let (width, height) = (641u32, 481u32);
        let tight = width as usize * 4;
        let stride = 2816usize;
        let mut data = vec![0u8; stride * height as usize];
        for row in 0..height as usize {
            data[row * stride..row * stride + tight].fill(255);
        }
        assert_eq!(infer_row_stride(data.len(), width, height, 4), Some(stride));

        let mut converter = I420Converter::new(PixelLayout::BGRA, 320, 240);
        let frame = converter
            .convert(&data, width, height, stride)
            .expect("valid frame");

        assert_eq!((frame.width, frame.height), (318, 240));
        assert_eq!(frame.y.len(), 318 * 240);
        assert_eq!(frame.u.len(), 159 * 120);
        assert!(frame.y.iter().all(|&y| y == 235), "padding leaked into Y");
        assert!(frame.u.iter().all(|&u| u == 128), "padding leaked into U");
        assert!(frame.v.iter().all(|&v| v == 128), "padding leaked into V");
    }

    #[test]
    fn known_colour_gives_expected_chroma() {
        // BT.601 pure red: Y=82, U=90, V=240; pure blue: Y=41, U=240, V=110.
        let red: Vec<u8> = [0u8, 0, 255, 255].repeat(6 * 4);
        let frame = convert_tight(PixelLayout::BGRA, &red, 6, 4);
        assert!(frame.y.iter().all(|&y| y == 82));
        assert!(frame.u.iter().all(|&u| u == 90), "{:?}", frame.u);
        assert!(frame.v.iter().all(|&v| v == 240), "{:?}", frame.v);

        let blue: Vec<u8> = [0u8, 0, 255].repeat(6 * 4);
        let frame = convert_tight(PixelLayout::RGB, &blue, 6, 4);
        assert!(frame.y.iter().all(|&y| y == 41), "{:?}", frame.y);
        assert!(frame.u.iter().all(|&u| u == 240), "{:?}", frame.u);
        assert!(frame.v.iter().all(|&v| v == 110), "{:?}", frame.v);
    }

    #[test]
    fn one_to_one_keeps_every_pixel_and_crops_odd_edge() {
        // 5x3 RGB frame with a distinct grey per pixel → 4x2, 1:1 (the odd
        // last column and row are dropped, not blended in).
        let (width, height) = (5u32, 3u32);
        let mut rgb = Vec::new();
        for i in 0..(width * height) {
            let v = (i * 16) as u8;
            rgb.extend_from_slice(&[v, v, v]);
        }
        let frame = convert_tight(PixelLayout::RGB, &rgb, width, height);
        assert_eq!((frame.width, frame.height), (4, 2));
        for row in 0..2usize {
            for col in 0..4usize {
                let v = i32::from(rgb[(row * 5 + col) * 3]);
                let expected = (((66 + 129 + 25) * v + 128) >> 8) + 16;
                assert_eq!(i32::from(frame.y[row * 4 + col]), expected, "({col},{row})");
            }
        }
    }

    #[test]
    fn one_pixel_frame_does_not_panic() {
        let frame = convert_tight(PixelLayout::BGRA, &[255, 255, 255, 255], 1, 1);
        assert_eq!((frame.width, frame.height), (2, 2));
        assert!(frame.y.iter().all(|&y| y == 235));
    }

    #[test]
    fn extreme_downscale_does_not_overflow() {
        // A single output pixel averaging a huge white box must stay white.
        let ranges_rows = (0usize, 4200usize);
        let ranges_cols = (0usize, 4200usize);
        let data = vec![255u8; 4200 * 4200 * 3];
        let (r, g, b) = box_average(&data, PixelLayout::RGB, 4200 * 3, ranges_rows, ranges_cols);
        assert_eq!((r, g, b), (255, 255, 255));
    }
}
