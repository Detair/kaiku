//! Lock-free sample ring between the PCM pusher thread and the output
//! callback.
//!
//! One producer (the pusher) and one consumer at a time (the output
//! callback). Neither side ever waits: the producer writes what fits, the
//! consumer reads what is there. Samples are stored as `f32` bits in
//! atomics (safe code; relaxed per-sample stores, published with
//! release/acquire on the indices). During a device switch two output
//! streams can briefly run at once; a consumer that finds the ring claimed
//! by the other one plays silence instead of waiting.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

pub struct SampleRing {
    buf: Box<[AtomicU32]>,
    /// Samples written so far (producer only).
    head: AtomicUsize,
    /// Samples read so far (consumer only).
    tail: AtomicUsize,
    /// Held while a consumer reads.
    reading: AtomicBool,
}

impl SampleRing {
    /// A ring holding at most `capacity` samples (allocated once).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: (0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            reading: AtomicBool::new(false),
        }
    }

    /// Samples waiting to be read.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append as many of `samples` as fit; returns how many were written.
    /// Single producer.
    pub fn push(&self, samples: &[f32]) -> usize {
        let cap = self.buf.len();
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let free = cap - head.wrapping_sub(tail);
        let n = free.min(samples.len());
        for (i, sample) in samples[..n].iter().enumerate() {
            self.buf[head.wrapping_add(i) % cap].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }

    /// Read up to `out.len()` samples into `out` (zeros instead when
    /// `silent`); returns how many were taken, or `None` when another
    /// consumer holds the ring right now. Never waits, never allocates.
    pub fn pop_into(&self, out: &mut [f32], silent: bool) -> Option<usize> {
        if self
            .reading
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        let cap = self.buf.len();
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let n = head.wrapping_sub(tail).min(out.len());
        for (i, o) in out[..n].iter_mut().enumerate() {
            let sample =
                f32::from_bits(self.buf[tail.wrapping_add(i) % cap].load(Ordering::Relaxed));
            *o = if silent { 0.0 } else { sample };
        }
        self.tail.store(tail.wrapping_add(n), Ordering::Release);
        self.reading.store(false, Ordering::Release);
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn samples_come_out_in_order_and_the_ring_never_overfills() {
        let ring = SampleRing::with_capacity(8);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(ring.push(&[4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]), 5);
        assert_eq!(ring.len(), 8);
        let mut out = [0.0; 5];
        assert_eq!(ring.pop_into(&mut out, false), Some(5));
        assert_eq!(
            out.map(f32::to_bits),
            [1.0f32, 2.0, 3.0, 4.0, 5.0].map(f32::to_bits)
        );
        assert_eq!(ring.push(&[11.0, 12.0]), 2);
        let mut out = [0.0; 10];
        assert_eq!(ring.pop_into(&mut out, false), Some(5));
        let got: Vec<u32> = out[..5].iter().map(|v| v.to_bits()).collect();
        let want: Vec<u32> = [6.0f32, 7.0, 8.0, 11.0, 12.0]
            .iter()
            .map(|v| v.to_bits())
            .collect();
        assert_eq!(got, want);
        assert!(ring.is_empty());
    }

    #[test]
    fn silent_reads_still_take_the_samples() {
        let ring = SampleRing::with_capacity(4);
        ring.push(&[0.5, 0.5]);
        let mut out = [9.0; 3];
        assert_eq!(ring.pop_into(&mut out, true), Some(2));
        assert_eq!(out.map(f32::to_bits), [0.0f32, 0.0, 9.0].map(f32::to_bits));
        assert!(ring.is_empty());
    }

    #[test]
    fn concurrent_producer_and_consumer_keep_every_sample_in_order() {
        let ring = Arc::new(SampleRing::with_capacity(1024));
        let producer = {
            let ring = ring.clone();
            std::thread::spawn(move || {
                let mut next = 0u32;
                while next < 200_000 {
                    let chunk: Vec<f32> = (next..next + 100).map(|v| v as f32).collect();
                    let written = ring.push(&chunk);
                    next += written as u32;
                    if written == 0 {
                        std::thread::yield_now();
                    }
                }
            })
        };
        let mut expected = 0u32;
        let mut out = [0.0f32; 97];
        while expected < 200_000 {
            if let Some(n) = ring.pop_into(&mut out, false) {
                for v in &out[..n] {
                    assert_eq!(v.to_bits(), (expected as f32).to_bits());
                    expected += 1;
                }
            }
        }
        producer.join().unwrap();
    }

    #[test]
    fn a_second_consumer_gets_none_instead_of_waiting() {
        let ring = SampleRing::with_capacity(4);
        ring.reading.store(true, Ordering::Relaxed); // the other stream is reading
        let mut out = [1.0; 2];
        assert_eq!(ring.pop_into(&mut out, false), None);
    }
}
