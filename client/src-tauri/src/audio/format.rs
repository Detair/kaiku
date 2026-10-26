//! Capture format negotiation and the capture front end.
//!
//! Real headsets do not always offer 48 kHz mono (many USB headsets only
//! expose 44.1 kHz stereo on their capture side). The capture stream opens
//! the device in a format it actually supports and this module converts the
//! interleaved device samples to the 48 kHz mono 20 ms frames that `RNNoise`
//! and Opus expect.

use super::{FRAME_SIZE, SAMPLE_RATE};

/// The format a capture stream is opened with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Pick the capture format from the device's supported configurations.
///
/// `supported` holds `(min_rate, max_rate, channels)` ranges. The caller puts
/// the device's default configuration FIRST: it is the fallback when no
/// range offers 48 kHz.
///
/// Preference: 48 kHz mono, else 48 kHz with the fewest channels, else the
/// device default (its rate clamped toward 48 kHz). An empty list yields
/// 48 kHz mono, which then fails to build and is reported to the caller.
pub fn negotiate_capture_format(supported: &[(u32, u32, u16)]) -> CaptureFormat {
    let offers_48k = |&&(min, max, channels): &&(u32, u32, u16)| {
        channels > 0 && min <= SAMPLE_RATE && SAMPLE_RATE <= max
    };

    if supported
        .iter()
        .filter(offers_48k)
        .any(|&(_, _, channels)| channels == 1)
    {
        return CaptureFormat {
            sample_rate: SAMPLE_RATE,
            channels: 1,
        };
    }

    if let Some(&(_, _, channels)) = supported
        .iter()
        .filter(offers_48k)
        .min_by_key(|&&(_, _, channels)| channels)
    {
        return CaptureFormat {
            sample_rate: SAMPLE_RATE,
            channels,
        };
    }

    match supported.iter().find(|&&(_, _, channels)| channels > 0) {
        Some(&(min, max, channels)) => CaptureFormat {
            sample_rate: SAMPLE_RATE.clamp(min, max.max(min)),
            channels,
        },
        None => CaptureFormat {
            sample_rate: SAMPLE_RATE,
            channels: 1,
        },
    }
}

/// Average interleaved frames down to one channel.
///
/// The capture callback uses the allocation-free `downmix_into` instead.
#[cfg_attr(not(test), allow(dead_code))]
pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let mut out = Vec::with_capacity(interleaved.len() / usize::from(channels.max(1)));
    downmix_into(interleaved, channels, &mut out);
    out
}

/// Non-allocating form of [`downmix_to_mono`] (appends to `out`; it only
/// grows when a callback delivers more samples than ever before).
fn downmix_into(interleaved: &[f32], channels: u16, out: &mut Vec<f32>) {
    let channels = usize::from(channels.max(1));
    if channels == 1 {
        out.extend_from_slice(interleaved);
        return;
    }
    let scale = 1.0 / channels as f32;
    out.extend(
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * scale),
    );
}

/// Streaming linear-interpolation resampler for mono voice.
///
/// Adds one input sample of latency (about 23 µs at 44.1 kHz) and does not
/// allocate beyond the output buffer it is given. When downsampling, a
/// one-pole low-pass sits in front; it only takes the edge off content above
/// the output Nyquist (≈6 dB/octave), it is not a real anti-alias filter.
/// Acceptable for voice ahead of Opus; a sinc resampler (e.g. `rubato`)
/// would replace this if real devices show artefacts.
#[derive(Debug, Clone)]
pub struct LinearResampler {
    /// Input samples advanced per output sample (`in_rate / out_rate`).
    step: f64,
    /// Position of the next output sample, in input samples, relative to
    /// `prev` (index 0 of the virtual array `[prev, input...]`).
    pos: f64,
    /// Last input sample of the previous call.
    prev: f32,
    /// Low-pass coefficient when downsampling, `None` otherwise.
    lowpass_alpha: Option<f32>,
    lowpass_state: f32,
}

impl LinearResampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let step = f64::from(in_rate) / f64::from(out_rate);
        let lowpass_alpha = (in_rate > out_rate).then(|| {
            // Cut-off at ~0.45 × output rate.
            let cutoff = 0.45 * f64::from(out_rate);
            (1.0 - (-2.0 * std::f64::consts::PI * cutoff / f64::from(in_rate)).exp()) as f32
        });
        Self {
            step,
            pos: 0.0,
            prev: 0.0,
            lowpass_alpha,
            lowpass_state: 0.0,
        }
    }

    /// Resample `input` and append the result to `out`.
    // `mul_add` is a libm call on targets without FMA; keep plain arithmetic
    // in the audio callback. The float loop bound is a fractional read position.
    #[allow(clippy::suboptimal_flops, clippy::while_float)]
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        let len = input.len();
        // Virtual array: v[0] = prev, v[k] = filtered input[k - 1].
        let mut filtered_prev = self.prev;
        let mut filtered_idx = 0usize; // number of input samples filtered so far
        let mut filtered_cur = self.prev;
        let last_index = len as f64; // v has len + 1 entries, last index = len
        while self.pos < last_index {
            let i = self.pos.floor() as usize;
            // Advance the filter so that `filtered_prev` = v[i], `filtered_cur` = v[i + 1].
            while filtered_idx <= i {
                filtered_prev = filtered_cur;
                filtered_cur = self.filter(input[filtered_idx]);
                filtered_idx += 1;
            }
            let frac = (self.pos - self.pos.floor()) as f32;
            out.push(filtered_prev + (filtered_cur - filtered_prev) * frac);
            self.pos += self.step;
        }
        // Run the remaining input through the filter so its state stays continuous.
        while filtered_idx < len {
            filtered_cur = self.filter(input[filtered_idx]);
            filtered_idx += 1;
        }
        self.prev = filtered_cur;
        self.pos -= last_index;
    }

    #[allow(clippy::suboptimal_flops)] // see `process`
    fn filter(&mut self, x: f32) -> f32 {
        match self.lowpass_alpha {
            Some(alpha) => {
                self.lowpass_state += alpha * (x - self.lowpass_state);
                self.lowpass_state
            }
            None => x,
        }
    }
}

/// Converts device-format capture callbacks into 48 kHz mono 20 ms frames.
///
/// All buffers are allocated up front; `push` runs inside the real-time
/// audio callback.
pub struct CaptureFrontEnd {
    channels: u16,
    resampler: Option<LinearResampler>,
    mono: Vec<f32>,
    resampled: Vec<f32>,
    frame: Vec<f32>,
}

/// Largest number of interleaved samples processed in one step. Bigger
/// callbacks are split into chunks of at most this size, so the scratch
/// buffers below are sized once and never grow in the audio callback.
pub const MAX_PUSH_SAMPLES: usize = 8192;

/// Interleaved chunk length for `channels`: the largest whole number of
/// frames that fits in [`MAX_PUSH_SAMPLES`].
pub fn push_chunk_len(channels: u16) -> usize {
    let channels = usize::from(channels.max(1));
    (MAX_PUSH_SAMPLES / channels).max(1) * channels
}

impl CaptureFrontEnd {
    pub fn new(format: CaptureFormat) -> Self {
        let channels = format.channels.max(1);
        let resampler = (format.sample_rate != SAMPLE_RATE)
            .then(|| LinearResampler::new(format.sample_rate, SAMPLE_RATE));
        // Worst case per chunk: every frame of a full chunk, upsampled.
        let mono_max = push_chunk_len(channels) / usize::from(channels);
        let resampled_max =
            (mono_max * SAMPLE_RATE as usize).div_ceil(format.sample_rate.max(1) as usize) + 4;
        Self {
            channels,
            resampler,
            mono: Vec::with_capacity(mono_max),
            resampled: Vec::with_capacity(resampled_max),
            frame: Vec::with_capacity(FRAME_SIZE),
        }
    }

    /// Feed one callback's interleaved samples; `on_frame` is called with
    /// every completed 960-sample (20 ms) 48 kHz mono frame.
    pub fn push(&mut self, interleaved: &[f32], mut on_frame: impl FnMut(&[f32])) {
        for chunk in interleaved.chunks(push_chunk_len(self.channels)) {
            self.push_chunk(chunk, &mut on_frame);
        }
    }

    fn push_chunk(&mut self, interleaved: &[f32], on_frame: &mut impl FnMut(&[f32])) {
        self.mono.clear();
        downmix_into(interleaved, self.channels, &mut self.mono);

        let samples: &[f32] = match self.resampler.as_mut() {
            Some(resampler) => {
                self.resampled.clear();
                resampler.process(&self.mono, &mut self.resampled);
                &self.resampled
            }
            None => &self.mono,
        };

        let mut rest = samples;
        while !rest.is_empty() {
            let take = (FRAME_SIZE - self.frame.len()).min(rest.len());
            self.frame.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.frame.len() == FRAME_SIZE {
                on_frame(&self.frame);
                self.frame.clear();
            }
        }
    }

    #[cfg(test)]
    const fn capacities(&self) -> (usize, usize, usize) {
        (
            self.mono.capacity(),
            self.resampled.capacity(),
            self.frame.capacity(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_48k_mono_when_available() {
        let format = negotiate_capture_format(&[
            (44_100, 44_100, 2),
            (8_000, 96_000, 2),
            (8_000, 96_000, 1),
        ]);
        assert_eq!(
            format,
            CaptureFormat {
                sample_rate: 48_000,
                channels: 1
            }
        );

        // No mono at 48 kHz: 48 kHz with the fewest channels wins over the default.
        let format = negotiate_capture_format(&[
            (44_100, 44_100, 1),
            (48_000, 48_000, 4),
            (48_000, 48_000, 2),
        ]);
        assert_eq!(
            format,
            CaptureFormat {
                sample_rate: 48_000,
                channels: 2
            }
        );
    }

    /// Sine at `freq` Hz, amplitude 0.5, interleaved over `channels`.
    fn sine_chunk(start: usize, frames: usize, rate: u32, channels: u16, freq: f32) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * usize::from(channels));
        for n in start..start + frames {
            let t = n as f32 / rate as f32;
            let s = 0.5 * (2.0 * std::f32::consts::PI * freq * t).sin();
            for _ in 0..channels {
                out.push(s);
            }
        }
        out
    }

    #[test]
    fn falls_back_to_device_default_and_resamples() {
        // A headset whose capture side only offers 44.1 kHz stereo.
        let format = negotiate_capture_format(&[(44_100, 44_100, 2)]);
        assert_eq!(
            format,
            CaptureFormat {
                sample_rate: 44_100,
                channels: 2
            }
        );

        let mut front_end = CaptureFrontEnd::new(format);
        let mut frames: Vec<Vec<f32>> = Vec::new();
        // One second in 10 ms callbacks (441 stereo frames each).
        for chunk in 0..100 {
            let data = sine_chunk(chunk * 441, 441, 44_100, 2, 440.0);
            front_end.push(&data, |frame| frames.push(frame.to_vec()));
        }

        assert!(frames.iter().all(|f| f.len() == 960), "48 kHz 20 ms frames");
        assert!(
            (49..=50).contains(&frames.len()),
            "1 s of audio → ~50 frames, got {}",
            frames.len()
        );

        // Audible: the tone survives with its level and pitch.
        let samples: Vec<f32> = frames.concat();
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        assert!((rms - 0.5 / 2f32.sqrt()).abs() < 0.02, "rms {rms}");
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        let seconds = samples.len() as f32 / 48_000.0;
        let freq = crossings as f32 / 2.0 / seconds;
        assert!((freq - 440.0).abs() < 5.0, "pitch {freq} Hz");
    }

    #[test]
    fn downmix_stereo_to_mono_averages() {
        assert_eq!(
            downmix_to_mono(&[1.0, 0.0, 0.5, -0.5, -1.0, -0.5], 2),
            vec![0.5, 0.0, -0.75]
        );
        assert_eq!(downmix_to_mono(&[0.25, -0.25], 1), vec![0.25, -0.25]);
    }

    #[test]
    fn native_48k_mono_passes_through_unchanged() {
        let mut front_end = CaptureFrontEnd::new(CaptureFormat {
            sample_rate: 48_000,
            channels: 1,
        });
        let input = sine_chunk(0, 960, 48_000, 1, 440.0);
        let mut frames: Vec<Vec<f32>> = Vec::new();
        front_end.push(&input[..500], |f| frames.push(f.to_vec()));
        assert!(frames.is_empty());
        front_end.push(&input[500..], |f| frames.push(f.to_vec()));
        assert_eq!(frames, vec![input]);
    }

    #[test]
    fn oversized_callbacks_do_not_grow_buffers() {
        for (rate, channels) in [(44_100, 8), (8_000, 1), (96_000, 2)] {
            let mut front_end = CaptureFrontEnd::new(CaptureFormat {
                sample_rate: rate,
                channels,
            });
            let before = front_end.capacities();
            // A huge ALSA period: far beyond the scratch size.
            let data = vec![0.1f32; MAX_PUSH_SAMPLES * 5 + usize::from(channels) * 3];
            let mut samples = 0usize;
            front_end.push(&data, |f| samples += f.len());
            assert_eq!(front_end.capacities(), before, "{rate} Hz × {channels}");
            assert!(samples > 0);
        }
    }

    #[test]
    fn downsampling_96k_keeps_pitch() {
        let format = negotiate_capture_format(&[(96_000, 96_000, 1)]);
        assert_eq!(format.sample_rate, 96_000);
        let mut front_end = CaptureFrontEnd::new(format);
        let mut samples = Vec::new();
        for chunk in 0..50 {
            let data = sine_chunk(chunk * 1920, 1920, 96_000, 1, 300.0);
            front_end.push(&data, |f| samples.extend_from_slice(f));
        }
        assert_eq!(samples.len() % 960, 0);
        assert!((samples.len() as i64 - 48_000).abs() <= 960);
        let crossings = samples
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        let freq = crossings as f32 / 2.0 / (samples.len() as f32 / 48_000.0);
        assert!((freq - 300.0).abs() < 5.0, "pitch {freq} Hz");
    }
}
