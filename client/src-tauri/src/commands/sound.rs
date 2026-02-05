//! Sound playback commands for notification sounds.

use rodio::{Decoder, OutputStreamBuilder, Sink};
use std::io::Cursor;
use std::thread;
use tauri::command;

// ============================================================================
// Sound Data (embedded)
// ============================================================================

// Sound files are embedded at compile time for reliability
static DEFAULT_SOUND: &[u8] = include_bytes!("../../resources/sounds/default.wav");
static SUBTLE_SOUND: &[u8] = include_bytes!("../../resources/sounds/subtle.wav");
static PING_SOUND: &[u8] = include_bytes!("../../resources/sounds/ping.wav");
static CHIME_SOUND: &[u8] = include_bytes!("../../resources/sounds/chime.wav");
static BELL_SOUND: &[u8] = include_bytes!("../../resources/sounds/bell.wav");

// ============================================================================
// Commands
// ============================================================================

/// Play a notification sound by ID.
///
/// Creates a new audio output stream per playback to avoid thread safety issues.
/// Volume is 0–100; defaults to 100 if not provided.
#[command]
pub fn play_sound(sound_id: String, volume: Option<u8>) -> Result<(), String> {
    let sound_data: &'static [u8] = match sound_id.as_str() {
        "default" => DEFAULT_SOUND,
        "subtle" => SUBTLE_SOUND,
        "ping" => PING_SOUND,
        "chime" => CHIME_SOUND,
        "bell" => BELL_SOUND,
        _ => {
            tracing::warn!("Unknown sound ID: {}, using default", sound_id);
            DEFAULT_SOUND
        }
    };

    let vol = volume.unwrap_or(100).min(100) as f32 / 100.0;

    // Spawn thread for audio playback (OutputStream is not Send)
    thread::spawn(move || {
        if let Err(e) = play_sound_blocking(sound_data, vol) {
            tracing::warn!("Failed to play sound: {}", e);
        }
    });

    Ok(())
}

/// Blocking sound playback (runs in dedicated thread).
fn play_sound_blocking(sound_data: &'static [u8], volume: f32) -> Result<(), String> {
    // Create audio output
    let stream = OutputStreamBuilder::open_default_stream()
        .map_err(|e| format!("Failed to open audio output: {e}"))?;

    // Create sink
    let sink = Sink::connect_new(stream.mixer());
    sink.set_volume(volume);

    // Decode and play
    let cursor = Cursor::new(sound_data);
    let source = Decoder::new(cursor).map_err(|e| format!("Failed to decode sound: {e}"))?;

    sink.append(source);
    sink.sleep_until_end();

    Ok(())
}

/// Get list of available sound IDs.
#[command]
pub fn get_available_sounds() -> Vec<String> {
    vec![
        "default".to_string(),
        "subtle".to_string(),
        "ping".to_string(),
        "chime".to_string(),
        "bell".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_available_sounds() {
        let sounds = get_available_sounds();
        assert_eq!(sounds.len(), 5);
        assert!(sounds.contains(&"default".to_string()));
    }
}
