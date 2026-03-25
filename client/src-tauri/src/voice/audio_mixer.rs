//! Audio Mixer
//!
//! Multi-track PCM mixer that combines decoded audio from all remote subscribers
//! into a single output stream for CPAL playback.
//!
//! Uses a **command channel** pattern so that `mpsc::Receiver`s stay local to the
//! mixer task and are never placed behind `Arc`/`RwLock`.

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::audio::{CHANNELS, FRAME_SIZE};

/// 20 ms stereo frame at 48 kHz = 960 samples/channel * 2 channels.
const FRAME_SAMPLES: usize = FRAME_SIZE * CHANNELS as usize;

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
}

impl AudioMixer {
    /// Create a new mixer that outputs mixed PCM to `output_tx`.
    ///
    /// Spawns the mixing background task and returns the mixer handle.
    pub fn new(output_tx: mpsc::Sender<Vec<f32>>) -> Self {
        let (command_tx, command_rx) = mpsc::channel(32);
        tokio::spawn(Self::run(command_rx, output_tx));
        Self { command_tx }
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
    async fn run(mut command_rx: mpsc::Receiver<MixerCommand>, output_tx: mpsc::Sender<Vec<f32>>) {
        let mut tracks: HashMap<String, mpsc::Receiver<Vec<f32>>> = HashMap::new();
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
            loop {
                match command_rx.try_recv() {
                    Ok(cmd) => Self::apply_command(cmd, &mut tracks),
                    Err(_) => break,
                }
            }

            // 2. Mix one frame from all tracks. For each track, drain all queued frames and keep
            //    only the latest to avoid accumulating latency.
            let mut mixed = vec![0.0f32; FRAME_SAMPLES];
            let mut active = 0;
            let mut closed_tracks = Vec::new();

            for (id, rx) in tracks.iter_mut() {
                let mut latest: Option<Vec<f32>> = None;

                // Drain channel — keep only the most recent frame.
                loop {
                    match rx.try_recv() {
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
                    for (i, sample) in pcm.iter().enumerate() {
                        if i < mixed.len() {
                            mixed[i] += sample;
                        }
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
    fn apply_command(cmd: MixerCommand, tracks: &mut HashMap<String, mpsc::Receiver<Vec<f32>>>) {
        match cmd {
            MixerCommand::AddTrack { track_id, pcm_rx } => {
                debug!(track_id = %track_id, "Mixer: added track");
                tracks.insert(track_id, pcm_rx);
            }
            MixerCommand::RemoveTrack { track_id } => {
                debug!(track_id = %track_id, "Mixer: removed track");
                tracks.remove(&track_id);
            }
        }
    }
}
