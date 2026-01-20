//! Voice statistics types and validation.
//!
//! This module provides types for tracking connection quality metrics
//! reported by clients during voice sessions.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Connection metrics reported by clients.
///
/// These stats are collected periodically from each participant
/// in a voice session to monitor connection quality.
#[derive(Debug, Clone, Deserialize)]
pub struct VoiceStats {
    /// The voice session ID this stat report belongs to.
    pub session_id: Uuid,
    /// Round-trip latency in milliseconds (0-10000).
    pub latency: i16,
    /// Packet loss percentage (0.0-100.0).
    pub packet_loss: f32,
    /// Jitter in milliseconds (0-5000).
    pub jitter: i16,
    /// Quality indicator (0=poor, 1=fair, 2=good, 3=excellent).
    pub quality: u8,
    /// Unix timestamp in milliseconds when the stats were collected.
    pub timestamp: i64,
}

impl VoiceStats {
    /// Validate stats are within acceptable ranges.
    ///
    /// Returns an error message if any field is out of range.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.latency < 0 || self.latency > 10000 {
            return Err("latency out of range (0-10000ms)");
        }
        if self.packet_loss < 0.0 || self.packet_loss > 100.0 {
            return Err("packet_loss out of range (0-100%)");
        }
        if self.jitter < 0 || self.jitter > 5000 {
            return Err("jitter out of range (0-5000ms)");
        }
        if self.quality > 3 {
            return Err("quality must be 0-3");
        }
        Ok(())
    }
}

/// Stats broadcast to other participants in the room.
///
/// This is a simplified version of `VoiceStats` that includes
/// the user ID for identification when broadcasting to peers.
#[derive(Debug, Clone, Serialize)]
pub struct UserStats {
    /// The user ID this stat belongs to.
    pub user_id: Uuid,
    /// Round-trip latency in milliseconds.
    pub latency: i16,
    /// Packet loss percentage.
    pub packet_loss: f32,
    /// Jitter in milliseconds.
    pub jitter: i16,
    /// Quality indicator (0=poor, 1=fair, 2=good, 3=excellent).
    pub quality: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_stats() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 1.5,
            jitter: 30,
            quality: 3,
            timestamp: 1234567890,
        };
        assert!(stats.validate().is_ok());
    }

    #[test]
    fn test_latency_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: -1,
            packet_loss: 0.0,
            jitter: 0,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("latency out of range (0-10000ms)"));

        let stats2 = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 10001,
            packet_loss: 0.0,
            jitter: 0,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats2.validate(), Err("latency out of range (0-10000ms)"));
    }

    #[test]
    fn test_packet_loss_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: -0.1,
            jitter: 30,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("packet_loss out of range (0-100%)"));

        let stats2 = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 100.1,
            jitter: 30,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats2.validate(), Err("packet_loss out of range (0-100%)"));
    }

    #[test]
    fn test_jitter_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 1.0,
            jitter: -1,
            quality: 3,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("jitter out of range (0-5000ms)"));
    }

    #[test]
    fn test_quality_out_of_range() {
        let stats = VoiceStats {
            session_id: Uuid::new_v4(),
            latency: 100,
            packet_loss: 1.0,
            jitter: 30,
            quality: 4,
            timestamp: 0,
        };
        assert_eq!(stats.validate(), Err("quality must be 0-3"));
    }
}
