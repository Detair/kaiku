//! Video Encoding and RTP Module
//!
//! VP8 software encoding and RTP packetization for screen sharing.

pub mod encoder;
pub mod rtp;

use thiserror::Error;

/// Video encoding errors.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum VideoError {
    #[error("Encoder initialization failed: {0}")]
    InitFailed(String),
    #[error("Encoding error: {0}")]
    EncodeFailed(String),
    #[error("RTP send error: {0}")]
    RtpSendFailed(String),
    #[error("Unsupported codec: {0}")]
    UnsupportedCodec(String),
}

/// An encoded video packet ready for RTP packetization.
pub struct EncodedPacket {
    /// Encoded data.
    pub data: Vec<u8>,
    /// Whether this is a keyframe.
    pub is_keyframe: bool,
    /// Presentation timestamp in 90kHz clock units.
    pub pts: u64,
}

/// Quality tier for screen sharing.
#[derive(Debug, Clone, Copy)]
pub struct QualityParams {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

impl QualityParams {
    /// Quality tiers for screen sharing.
    pub fn from_tier(tier: &str) -> Result<Self, String> {
        match tier {
            "low" => Ok(Self {
                width: 854,
                height: 480,
                fps: 15,
                bitrate_kbps: 500,
            }),
            "medium" => Ok(Self {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate_kbps: 1500,
            }),
            "high" => Ok(Self {
                width: 1920,
                height: 1080,
                fps: 30,
                bitrate_kbps: 3000,
            }),
            "premium" => Ok(Self {
                width: 1920,
                height: 1080,
                fps: 60,
                bitrate_kbps: 5000,
            }),
            _ => Err(format!("Unknown quality tier: {tier}")),
        }
    }

    /// Quality tiers for webcam capture (lower bitrates than screen sharing).
    pub fn from_webcam_tier(tier: &str) -> Result<Self, String> {
        match tier {
            "low" => Ok(Self {
                width: 320,
                height: 240,
                fps: 15,
                bitrate_kbps: 200,
            }),
            "medium" => Ok(Self {
                width: 640,
                height: 480,
                fps: 30,
                bitrate_kbps: 500,
            }),
            "high" => Ok(Self {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate_kbps: 1000,
            }),
            "premium" => Ok(Self {
                width: 1920,
                height: 1080,
                fps: 30,
                bitrate_kbps: 2000,
            }),
            _ => Err(format!("Unknown webcam quality tier: {tier}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_tier_valid_tiers() {
        let low = QualityParams::from_tier("low").unwrap();
        assert_eq!(low.width, 854);
        assert_eq!(low.fps, 15);

        let medium = QualityParams::from_tier("medium").unwrap();
        assert_eq!(medium.width, 1280);
        assert_eq!(medium.fps, 30);

        let high = QualityParams::from_tier("high").unwrap();
        assert_eq!(high.width, 1920);
        assert_eq!(high.bitrate_kbps, 3000);

        let premium = QualityParams::from_tier("premium").unwrap();
        assert_eq!(premium.fps, 60);
        assert_eq!(premium.bitrate_kbps, 5000);
    }

    #[test]
    fn from_tier_invalid_returns_error() {
        assert!(QualityParams::from_tier("ultra").is_err());
        assert!(QualityParams::from_tier("").is_err());
        assert!(QualityParams::from_tier("Medium").is_err());
    }

    #[test]
    fn from_webcam_tier_valid_tiers() {
        let low = QualityParams::from_webcam_tier("low").unwrap();
        assert_eq!(low.width, 320);
        assert_eq!(low.height, 240);
        assert_eq!(low.fps, 15);
        assert_eq!(low.bitrate_kbps, 200);

        let medium = QualityParams::from_webcam_tier("medium").unwrap();
        assert_eq!(medium.width, 640);
        assert_eq!(medium.fps, 30);

        let high = QualityParams::from_webcam_tier("high").unwrap();
        assert_eq!(high.width, 1280);
        assert_eq!(high.bitrate_kbps, 1000);

        let premium = QualityParams::from_webcam_tier("premium").unwrap();
        assert_eq!(premium.width, 1920);
        assert_eq!(premium.bitrate_kbps, 2000);
    }

    #[test]
    fn from_webcam_tier_invalid_returns_error() {
        assert!(QualityParams::from_webcam_tier("ultra").is_err());
        assert!(QualityParams::from_webcam_tier("").is_err());
    }
}
