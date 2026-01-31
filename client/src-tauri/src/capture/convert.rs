//! Color Space Conversion
//!
//! BGRA → I420 (YUV 4:2:0 planar) using BT.601 coefficients.
//! Reuses output buffers to avoid per-frame allocation.

use super::I420Frame;

/// Reusable converter that avoids per-frame allocation.
pub struct BgraToI420Converter {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
    width: u32,
    height: u32,
}

impl BgraToI420Converter {
    /// Create a new converter pre-allocated for the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        let y_size = (width * height) as usize;
        let uv_size = ((width / 2) * (height / 2)) as usize;

        Self {
            y: vec![0u8; y_size],
            u: vec![0u8; uv_size],
            v: vec![0u8; uv_size],
            width,
            height,
        }
    }

    /// Convert a BGRA frame to an owned `I420Frame`.
    ///
    /// This clones the internal buffers. For the capture pipeline, this is called once per frame
    /// and the clone is cheaper than re-allocation since the Vec capacity is reused.
    #[allow(clippy::many_single_char_names)]
    pub fn convert_owned(&mut self, bgra: &[u8]) -> I420Frame {
        let w = self.width as usize;
        let h = self.height as usize;

        debug_assert_eq!(bgra.len(), w * h * 4);

        if self.y.len() != w * h {
            self.y.resize(w * h, 0);
            self.u.resize((w / 2) * (h / 2), 0);
            self.v.resize((w / 2) * (h / 2), 0);
            self.width = w as u32;
            self.height = h as u32;
        }

        let uv_width = w / 2;

        for row in (0..h).step_by(2) {
            for col in (0..w).step_by(2) {
                let mut sum_u: i32 = 0;
                let mut sum_v: i32 = 0;

                for dy in 0..2usize {
                    let y_row = row + dy;
                    if y_row >= h {
                        break;
                    }
                    for dx in 0..2usize {
                        let x_col = col + dx;
                        if x_col >= w {
                            break;
                        }

                        let px = (y_row * w + x_col) * 4;
                        let b = i32::from(bgra[px]);
                        let g = i32::from(bgra[px + 1]);
                        let r = i32::from(bgra[px + 2]);

                        let y_val = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
                        self.y[y_row * w + x_col] = y_val.clamp(0, 255) as u8;

                        sum_u += (-38 * r - 74 * g + 112 * b + 128) >> 8;
                        sum_v += (112 * r - 94 * g - 18 * b + 128) >> 8;
                    }
                }

                let uv_idx = (row / 2) * uv_width + (col / 2);
                self.u[uv_idx] = ((sum_u / 4) + 128).clamp(0, 255) as u8;
                self.v[uv_idx] = ((sum_v / 4) + 128).clamp(0, 255) as u8;
            }
        }

        I420Frame {
            y: self.y.clone(),
            u: self.u.clone(),
            v: self.v.clone(),
            width: self.width,
            height: self.height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_black_frame() {
        let width = 4u32;
        let height = 4u32;
        // Black BGRA: all zeros (alpha = 255)
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        for i in (0..bgra.len()).step_by(4) {
            bgra[i + 3] = 255; // alpha
        }

        let mut converter = BgraToI420Converter::new(width, height);
        let frame = converter.convert_owned(&bgra);

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

        let mut converter = BgraToI420Converter::new(width, height);
        let frame = converter.convert_owned(&bgra);

        // White should produce Y=235 in BT.601
        for &y in &frame.y {
            assert_eq!(y, 235, "Y should be 235 for white");
        }
    }
}
