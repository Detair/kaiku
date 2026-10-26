//! Audio Mixer
//!
//! Multi-track PCM mixer that combines decoded audio from all remote subscribers
//! into a single output stream for CPAL playback.
//!
//! Uses a **command channel** pattern so that `mpsc::Receiver`s stay local to the
//! mixer task and are never placed behind `Arc`/`RwLock`.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info};
use uuid::Uuid;

use crate::audio::{CHANNELS, FRAME_SIZE};

/// 20 ms stereo frame at 48 kHz = 960 samples/channel * 2 channels.
const FRAME_SAMPLES: usize = FRAME_SIZE * CHANNELS as usize;

/// Highest per-user gain (200 %).
pub const MAX_USER_GAIN: f32 = 2.0;

/// Per-person playback gains, keyed by user id (1.0 when unset, 0.0 = muted).
///
/// Shared between the voice state and every mixer, so a volume chosen for
/// someone applies to all of their audio tracks and outlives a single call.
/// Written from commands, read by the mixer task once per 20 ms tick — never
/// from the real-time audio callback.
#[derive(Clone, Default)]
pub struct UserGains(Arc<RwLock<HashMap<Uuid, f32>>>);

impl UserGains {
    /// Set `user_id`'s gain, clamped to `0.0..=MAX_USER_GAIN`.
    pub fn set(&self, user_id: Uuid, gain: f32) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, MAX_USER_GAIN)
        } else {
            1.0
        };
        if let Ok(mut gains) = self.0.write() {
            if (gain - 1.0).abs() < f32::EPSILON {
                gains.remove(&user_id);
            } else {
                gains.insert(user_id, gain);
            }
        }
    }

    /// Gain for `user_id` (1.0 when never set).
    pub fn get(&self, user_id: Uuid) -> f32 {
        self.0
            .read()
            .ok()
            .and_then(|gains| gains.get(&user_id).copied())
            .unwrap_or(1.0)
    }
}

/// A registered remote track: its PCM receiver and the user it belongs to.
struct Track {
    pcm_rx: mpsc::Receiver<Vec<f32>>,
    /// Parsed from the `"{user_id}:{source}"` track id; `None` → unity gain.
    user_id: Option<Uuid>,
}

/// Commands sent to the mixer run loop.
enum MixerCommand {
    AddTrack {
        track_id: String,
        pcm_rx: mpsc::Receiver<Vec<f32>>,
    },
    RemoveTrack {
        track_id: String,
    },
}

/// Handle for the background audio mixer task.
///
/// Cloneable and Send — callers interact with the mixer exclusively through
/// the command channel.
#[derive(Clone)]
pub struct AudioMixer {
    command_tx: mpsc::Sender<MixerCommand>,
    gains: UserGains,
}

impl AudioMixer {
    /// Create a new mixer that outputs mixed PCM to `output_tx`.
    ///
    /// Spawns the mixing background task and returns the mixer handle.
    pub fn new(output_tx: mpsc::Sender<Vec<f32>>) -> Self {
        Self::with_gains(output_tx, UserGains::default())
    }

    /// Like [`AudioMixer::new`], applying the shared per-user `gains`.
    pub fn with_gains(output_tx: mpsc::Sender<Vec<f32>>, gains: UserGains) -> Self {
        let (command_tx, command_rx) = mpsc::channel(32);
        tokio::spawn(Self::run(command_rx, output_tx, gains.clone()));
        Self { command_tx, gains }
    }

    /// Set how loud `user_id` plays (1.0 = unchanged, 0.0 = muted, max 2.0).
    /// Applies to all of the user's audio tracks from the next 20 ms frame.
    pub fn set_user_gain(&self, user_id: Uuid, gain: f32) {
        self.gains.set(user_id, gain);
    }

    /// Register a new audio track. Returns the sender to feed PCM samples to.
    pub async fn add_track(&self, track_id: String) -> mpsc::Sender<Vec<f32>> {
        let (pcm_tx, pcm_rx) = mpsc::channel(64);
        let _ = self
            .command_tx
            .send(MixerCommand::AddTrack { track_id, pcm_rx })
            .await;
        pcm_tx
    }

    /// Remove a track from the mixer.
    pub async fn remove_track(&self, track_id: String) {
        let _ = self
            .command_tx
            .send(MixerCommand::RemoveTrack { track_id })
            .await;
    }

    /// Background mixing loop.
    ///
    /// Owns all track receivers — they never leave this task.
    /// Uses a 20ms interval timer for pacing instead of busy-polling.
    async fn run(
        mut command_rx: mpsc::Receiver<MixerCommand>,
        output_tx: mpsc::Sender<Vec<f32>>,
        gains: UserGains,
    ) {
        let mut tracks: HashMap<String, Track> = HashMap::new();
        let mut ticker = tokio::time::interval(Duration::from_millis(20));

        loop {
            // When no tracks are registered, block-wait for the next command
            // instead of spinning the interval timer.
            if tracks.is_empty() {
                match command_rx.recv().await {
                    Some(cmd) => {
                        Self::apply_command(cmd, &mut tracks);
                        continue;
                    }
                    None => break, // mixer handle dropped
                }
            }

            // Wait for the next 20ms tick.
            ticker.tick().await;

            // 1. Process any pending commands (non-blocking).
            while let Ok(cmd) = command_rx.try_recv() {
                Self::apply_command(cmd, &mut tracks);
            }

            // 2. Mix one frame from all tracks. For each track, drain all queued frames and keep
            //    only the latest to avoid accumulating latency.
            let mut mixed = vec![0.0f32; FRAME_SAMPLES];
            let mut active = 0;
            let mut closed_tracks = Vec::new();

            for (id, track) in &mut tracks {
                let mut latest: Option<Vec<f32>> = None;

                // Drain channel — keep only the most recent frame.
                loop {
                    match track.pcm_rx.try_recv() {
                        Ok(pcm) => {
                            latest = Some(pcm);
                        }
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            if latest.is_none() {
                                // Producer dropped — mark for removal.
                                closed_tracks.push(id.clone());
                            }
                            break;
                        }
                        Err(mpsc::error::TryRecvError::Empty) => break,
                    }
                }

                if let Some(pcm) = latest {
                    let gain = track.user_id.map_or(1.0, |user| gains.get(user));
                    for (out, sample) in mixed.iter_mut().zip(pcm.iter()) {
                        *out += sample * gain;
                    }
                    active += 1;
                }
            }

            // Remove dead tracks.
            for id in closed_tracks {
                debug!(track_id = %id, "Mixer: track producer disconnected, removing");
                tracks.remove(&id);
            }

            if active > 0 {
                // Soft-clip to prevent distortion.
                for sample in mixed.iter_mut() {
                    *sample = sample.clamp(-1.0, 1.0);
                }
                if output_tx.send(mixed).await.is_err() {
                    break; // playback dropped
                }
            }
        }

        info!("Mixer: run loop exited");
    }

    /// Apply a single command to the track map.
    fn apply_command(cmd: MixerCommand, tracks: &mut HashMap<String, Track>) {
        match cmd {
            MixerCommand::AddTrack { track_id, pcm_rx } => {
                debug!(track_id = %track_id, "Mixer: added track");
                let user_id = track_id
                    .split(':')
                    .next()
                    .and_then(|user| Uuid::parse_str(user).ok());
                tracks.insert(track_id, Track { pcm_rx, user_id });
            }
            MixerCommand::RemoveTrack { track_id } => {
                debug!(track_id = %track_id, "Mixer: removed track");
                tracks.remove(&track_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use uuid::Uuid;

    use super::*;

    async fn mix_one(gain: Option<f32>, sample: f32) -> Vec<f32> {
        let (out_tx, mut out_rx) = mpsc::channel(8);
        let mixer = AudioMixer::new(out_tx);
        let user = Uuid::new_v4();
        if let Some(gain) = gain {
            mixer.set_user_gain(user, gain);
        }
        let pcm_tx = mixer.add_track(format!("{user}:microphone")).await;
        pcm_tx.send(vec![sample; FRAME_SAMPLES]).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), out_rx.recv())
            .await
            .expect("mixer produced a frame")
            .expect("mixer alive")
    }

    #[tokio::test]
    async fn mixer_applies_user_gain() {
        let unity = mix_one(None, 0.4).await;
        assert!(unity.iter().all(|s| (s - 0.4).abs() < 1e-6));

        let half = mix_one(Some(0.5), 0.4).await;
        assert!(
            half.iter().all(|s| (s - 0.2).abs() < 1e-6),
            "{:?}",
            &half[..4]
        );

        // Boost is allowed but the mix stays clipped to [-1, 1].
        let boosted = mix_one(Some(2.0), 0.8).await;
        assert!(boosted.iter().all(|s| (s - 1.0).abs() < 1e-6));
    }

    #[tokio::test]
    async fn zero_gain_is_silent() {
        let muted = mix_one(Some(0.0), 0.9).await;
        assert_eq!(muted.len(), FRAME_SAMPLES);
        assert!(muted.iter().all(|s| *s == 0.0));
    }

    #[tokio::test]
    async fn gain_is_per_user_and_survives_a_new_mixer() {
        let gains = UserGains::default();
        let (out_tx, mut out_rx) = mpsc::channel(8);
        let quiet = Uuid::new_v4();
        let loud = Uuid::new_v4();
        gains.set(quiet, 0.0);
        let mixer = AudioMixer::with_gains(out_tx, gains.clone());
        let quiet_tx = mixer.add_track(format!("{quiet}:microphone")).await;
        let loud_tx = mixer.add_track(format!("{loud}:microphone")).await;
        quiet_tx.send(vec![0.5; FRAME_SAMPLES]).await.unwrap();
        loud_tx.send(vec![0.25; FRAME_SAMPLES]).await.unwrap();
        // Frames may land on different ticks; wait until both were mixed once.
        let mut seen = Vec::new();
        for _ in 0..2 {
            if let Ok(Some(frame)) =
                tokio::time::timeout(Duration::from_millis(500), out_rx.recv()).await
            {
                seen.push(frame[0]);
            }
        }
        // The muted user never reaches the mix: each frame is either silence
        // (only the muted track arrived that tick) or the other user alone.
        assert!(seen.iter().any(|s| (s - 0.25).abs() < 1e-6), "{seen:?}");
        assert!(
            seen.iter()
                .all(|s| s.abs() < 1e-6 || (s - 0.25).abs() < 1e-6),
            "{seen:?}"
        );
        assert!(gains.get(quiet).abs() < f32::EPSILON);
    }
}
