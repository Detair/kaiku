# Screen Sharing Phase 1: Foundation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the foundational data structures and permissions required for screen sharing.

**Architecture:** Extend the existing voice SFU to support multiple tracks per peer (audio + video). Add permission bits for screen sharing and user-level feature flags for premium video.

**Tech Stack:** Rust (server), SQLx migrations, Redis, bitflags crate

**Design Doc:** `docs/plans/2026-01-19-screen-sharing-design.md`

**Working Directory:** `/home/detair/GIT/canis/.worktrees/screen-sharing`

**Build Command:** `SQLX_OFFLINE=true cargo build --workspace`

---

## Task 1: Add TrackSource and TrackKind Enums

**Files:**
- Create: `server/src/voice/track_types.rs`
- Modify: `server/src/voice/mod.rs`

**Step 1: Create the track types module**

Create `server/src/voice/track_types.rs`:

```rust
//! Track metadata types for SFU multi-track support.
//!
//! Each peer can have multiple tracks (mic, screen video, screen audio, webcam).
//! These types identify and route tracks correctly.

use serde::{Deserialize, Serialize};

/// The kind of media track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    /// Audio track (mic, screen audio)
    Audio,
    /// Video track (screen share, webcam)
    Video,
}

/// The source/purpose of a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackSource {
    /// User's microphone
    Microphone,
    /// Screen share video stream
    ScreenVideo,
    /// Screen share audio (system/app audio)
    ScreenAudio,
    /// Webcam video (future)
    Webcam,
}

impl TrackSource {
    /// Returns the track kind for this source.
    pub fn kind(&self) -> TrackKind {
        match self {
            TrackSource::Microphone => TrackKind::Audio,
            TrackSource::ScreenVideo => TrackKind::Video,
            TrackSource::ScreenAudio => TrackKind::Audio,
            TrackSource::Webcam => TrackKind::Video,
        }
    }

    /// Returns true if this is a video source.
    pub fn is_video(&self) -> bool {
        matches!(self.kind(), TrackKind::Video)
    }

    /// Returns true if this is an audio source.
    pub fn is_audio(&self) -> bool {
        matches!(self.kind(), TrackKind::Audio)
    }
}

/// Metadata about a track in a voice session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackInfo {
    /// Unique track identifier (from WebRTC)
    pub track_id: String,
    /// User who owns this track
    pub user_id: uuid::Uuid,
    /// Kind of track (audio/video)
    pub kind: TrackKind,
    /// Source of the track
    pub source: TrackSource,
    /// Codec being used (e.g., "opus", "vp9", "h264")
    pub codec: String,
    /// Human-readable label (e.g., "Display 1", "Firefox")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl TrackInfo {
    /// Create a new TrackInfo for a microphone track.
    pub fn microphone(track_id: String, user_id: uuid::Uuid) -> Self {
        Self {
            track_id,
            user_id,
            kind: TrackKind::Audio,
            source: TrackSource::Microphone,
            codec: "opus".to_string(),
            label: None,
        }
    }

    /// Create a new TrackInfo for a screen share video track.
    pub fn screen_video(
        track_id: String,
        user_id: uuid::Uuid,
        codec: String,
        label: Option<String>,
    ) -> Self {
        Self {
            track_id,
            user_id,
            kind: TrackKind::Video,
            source: TrackSource::ScreenVideo,
            codec,
            label,
        }
    }

    /// Create a new TrackInfo for screen share audio track.
    pub fn screen_audio(track_id: String, user_id: uuid::Uuid) -> Self {
        Self {
            track_id,
            user_id,
            kind: TrackKind::Audio,
            source: TrackSource::ScreenAudio,
            codec: "opus".to_string(),
            label: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_source_kind() {
        assert_eq!(TrackSource::Microphone.kind(), TrackKind::Audio);
        assert_eq!(TrackSource::ScreenVideo.kind(), TrackKind::Video);
        assert_eq!(TrackSource::ScreenAudio.kind(), TrackKind::Audio);
        assert_eq!(TrackSource::Webcam.kind(), TrackKind::Video);
    }

    #[test]
    fn test_track_source_is_video() {
        assert!(!TrackSource::Microphone.is_video());
        assert!(TrackSource::ScreenVideo.is_video());
        assert!(!TrackSource::ScreenAudio.is_video());
        assert!(TrackSource::Webcam.is_video());
    }

    #[test]
    fn test_track_info_microphone() {
        let user_id = uuid::Uuid::new_v4();
        let info = TrackInfo::microphone("track-123".to_string(), user_id);

        assert_eq!(info.source, TrackSource::Microphone);
        assert_eq!(info.kind, TrackKind::Audio);
        assert_eq!(info.codec, "opus");
        assert!(info.label.is_none());
    }

    #[test]
    fn test_track_info_screen_video() {
        let user_id = uuid::Uuid::new_v4();
        let info = TrackInfo::screen_video(
            "track-456".to_string(),
            user_id,
            "vp9".to_string(),
            Some("Display 1".to_string()),
        );

        assert_eq!(info.source, TrackSource::ScreenVideo);
        assert_eq!(info.kind, TrackKind::Video);
        assert_eq!(info.codec, "vp9");
        assert_eq!(info.label, Some("Display 1".to_string()));
    }
}
```

**Step 2: Export from voice module**

Add to `server/src/voice/mod.rs` after existing exports:

```rust
mod track_types;
pub use track_types::{TrackInfo, TrackKind, TrackSource};
```

**Step 3: Run tests**

```bash
cd /home/detair/GIT/canis/.worktrees/screen-sharing
SQLX_OFFLINE=true cargo test track_types --workspace -- --nocapture
```

Expected: 4 tests passing

**Step 4: Commit**

```bash
git add server/src/voice/track_types.rs server/src/voice/mod.rs
git commit -m "feat(voice): add TrackSource and TrackKind enums for multi-track support"
```

---

## Task 2: Add SCREEN_SHARE Permission Bit

**Files:**
- Modify: `server/src/permissions/guild.rs`
- Modify: `server/src/permissions/mod.rs` (if needed)

**Step 1: Read the current permission definitions**

Check `server/src/permissions/guild.rs` for the existing permission bits and find the next available bit.

**Step 2: Add SCREEN_SHARE permission**

Add to the `GuildPermissions` bitflags in `server/src/permissions/guild.rs`:

```rust
/// Can start screen sharing in voice channels
const SCREEN_SHARE = 1 << 22;
```

**Step 3: Update permission groups if they exist**

If there's a `DEFAULT_PERMISSIONS` or `ALL_PERMISSIONS` constant, ensure `SCREEN_SHARE` is included appropriately:
- Should be in `ALL_PERMISSIONS`
- Should NOT be in default @everyone (moderator+ feature)

**Step 4: Add to permission display/descriptions**

If there's a function that returns permission names or descriptions, add:

```rust
GuildPermissions::SCREEN_SHARE => "Screen Share",
```

**Step 5: Run tests**

```bash
SQLX_OFFLINE=true cargo test permissions --workspace -- --nocapture
```

Expected: All permission tests pass

**Step 6: Commit**

```bash
git add server/src/permissions/
git commit -m "feat(permissions): add SCREEN_SHARE permission bit (bit 22)"
```

---

## Task 3: Add User Feature Flags Column

**Files:**
- Create: `server/migrations/YYYYMMDDHHMMSS_add_user_feature_flags.sql`
- Modify: `server/src/db/users.rs` (User struct)

**Step 1: Create migration file**

Create migration with timestamp (use current time):

```bash
cd /home/detair/GIT/canis/.worktrees/screen-sharing/server
touch migrations/$(date +%Y%m%d%H%M%S)_add_user_feature_flags.sql
```

Content:

```sql
-- Add feature flags column to users table
-- Bit 0: PREMIUM_VIDEO (1080p60 screen sharing)
-- Future bits reserved for additional premium features

ALTER TABLE users ADD COLUMN feature_flags BIGINT NOT NULL DEFAULT 0;

-- Add comment explaining the flags
COMMENT ON COLUMN users.feature_flags IS 'User-level feature flags. Bit 0: PREMIUM_VIDEO';
```

**Step 2: Add UserFeatures bitflags**

Create or add to `server/src/db/users.rs` (or appropriate location):

```rust
use bitflags::bitflags;

bitflags! {
    /// User-level feature flags for premium features.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct UserFeatures: i64 {
        /// Premium video quality (1080p60) for screen sharing
        const PREMIUM_VIDEO = 1 << 0;
        // Future flags:
        // const PREMIUM_AUDIO = 1 << 1;
        // const CUSTOM_THEMES = 1 << 2;
    }
}

impl UserFeatures {
    /// Check if user has premium video feature.
    pub fn has_premium_video(&self) -> bool {
        self.contains(UserFeatures::PREMIUM_VIDEO)
    }
}
```

**Step 3: Update User struct**

Add to the `User` struct:

```rust
pub feature_flags: i64,
```

**Step 4: Update user queries**

Find all `SELECT` queries for users and add `feature_flags` to the column list.

**Step 5: Regenerate SQLx offline data**

```bash
# This requires a running database - skip if not available
# sqlx prepare --workspace
```

If database not available, manually add the query JSON to `.sqlx/` directory.

**Step 6: Commit**

```bash
git add server/migrations/ server/src/db/
git commit -m "feat(db): add user feature_flags column for premium features"
```

---

## Task 4: Add Quality Enum

**Files:**
- Create: `server/src/voice/quality.rs`
- Modify: `server/src/voice/mod.rs`

**Step 1: Create quality module**

Create `server/src/voice/quality.rs`:

```rust
//! Video quality tiers for screen sharing.

use serde::{Deserialize, Serialize};

/// Video quality tier for screen sharing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// 480p @ 15fps, 0.5-1 Mbps - fallback for poor connections
    Low,
    /// 720p @ 30fps, 1.5-3 Mbps - default quality
    #[default]
    Medium,
    /// 1080p @ 30fps, 3-5 Mbps - good connections
    High,
    /// 1080p @ 60fps, 4-8 Mbps - requires PREMIUM_VIDEO
    Premium,
}

impl Quality {
    /// Maximum width for this quality tier.
    pub fn max_width(&self) -> u32 {
        match self {
            Quality::Low => 854,
            Quality::Medium => 1280,
            Quality::High => 1920,
            Quality::Premium => 1920,
        }
    }

    /// Maximum height for this quality tier.
    pub fn max_height(&self) -> u32 {
        match self {
            Quality::Low => 480,
            Quality::Medium => 720,
            Quality::High => 1080,
            Quality::Premium => 1080,
        }
    }

    /// Maximum framerate for this quality tier.
    pub fn max_fps(&self) -> u32 {
        match self {
            Quality::Low => 15,
            Quality::Medium => 30,
            Quality::High => 30,
            Quality::Premium => 60,
        }
    }

    /// Target bitrate in bits per second.
    pub fn target_bitrate(&self) -> u32 {
        match self {
            Quality::Low => 750_000,      // 750 kbps
            Quality::Medium => 2_000_000,  // 2 Mbps
            Quality::High => 4_000_000,    // 4 Mbps
            Quality::Premium => 6_000_000, // 6 Mbps
        }
    }

    /// Maximum bitrate in bits per second.
    pub fn max_bitrate(&self) -> u32 {
        match self {
            Quality::Low => 1_000_000,     // 1 Mbps
            Quality::Medium => 3_000_000,  // 3 Mbps
            Quality::High => 5_000_000,    // 5 Mbps
            Quality::Premium => 8_000_000, // 8 Mbps
        }
    }

    /// Returns true if this quality requires premium feature flag.
    pub fn requires_premium(&self) -> bool {
        matches!(self, Quality::Premium)
    }

    /// Downgrade to the next lower quality tier.
    pub fn downgrade(&self) -> Quality {
        match self {
            Quality::Premium => Quality::High,
            Quality::High => Quality::Medium,
            Quality::Medium => Quality::Low,
            Quality::Low => Quality::Low, // Can't go lower
        }
    }

    /// Upgrade to the next higher quality tier (up to max).
    pub fn upgrade(&self, max: Quality) -> Quality {
        let upgraded = match self {
            Quality::Low => Quality::Medium,
            Quality::Medium => Quality::High,
            Quality::High => Quality::Premium,
            Quality::Premium => Quality::Premium,
        };

        // Don't exceed max allowed
        if upgraded.max_bitrate() > max.max_bitrate() {
            *self
        } else {
            upgraded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_dimensions() {
        assert_eq!(Quality::Low.max_height(), 480);
        assert_eq!(Quality::Medium.max_height(), 720);
        assert_eq!(Quality::High.max_height(), 1080);
        assert_eq!(Quality::Premium.max_height(), 1080);
    }

    #[test]
    fn test_quality_fps() {
        assert_eq!(Quality::Low.max_fps(), 15);
        assert_eq!(Quality::Medium.max_fps(), 30);
        assert_eq!(Quality::High.max_fps(), 30);
        assert_eq!(Quality::Premium.max_fps(), 60);
    }

    #[test]
    fn test_quality_premium_requirement() {
        assert!(!Quality::Low.requires_premium());
        assert!(!Quality::Medium.requires_premium());
        assert!(!Quality::High.requires_premium());
        assert!(Quality::Premium.requires_premium());
    }

    #[test]
    fn test_quality_downgrade() {
        assert_eq!(Quality::Premium.downgrade(), Quality::High);
        assert_eq!(Quality::High.downgrade(), Quality::Medium);
        assert_eq!(Quality::Medium.downgrade(), Quality::Low);
        assert_eq!(Quality::Low.downgrade(), Quality::Low);
    }

    #[test]
    fn test_quality_upgrade() {
        assert_eq!(Quality::Low.upgrade(Quality::Premium), Quality::Medium);
        assert_eq!(Quality::Medium.upgrade(Quality::Premium), Quality::High);
        assert_eq!(Quality::High.upgrade(Quality::Premium), Quality::Premium);

        // Respects max
        assert_eq!(Quality::Medium.upgrade(Quality::High), Quality::High);
        assert_eq!(Quality::High.upgrade(Quality::High), Quality::High);
    }

    #[test]
    fn test_quality_default() {
        assert_eq!(Quality::default(), Quality::Medium);
    }
}
```

**Step 2: Export from voice module**

Add to `server/src/voice/mod.rs`:

```rust
mod quality;
pub use quality::Quality;
```

**Step 3: Run tests**

```bash
SQLX_OFFLINE=true cargo test quality --workspace -- --nocapture
```

Expected: 6 tests passing

**Step 4: Commit**

```bash
git add server/src/voice/quality.rs server/src/voice/mod.rs
git commit -m "feat(voice): add Quality enum for screen share quality tiers"
```

---

## Task 5: Add ScreenShareInfo Struct

**Files:**
- Create: `server/src/voice/screen_share.rs`
- Modify: `server/src/voice/mod.rs`

**Step 1: Create screen share types module**

Create `server/src/voice/screen_share.rs`:

```rust
//! Screen sharing data types and state.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::Quality;

/// Information about an active screen share session.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScreenShareInfo {
    /// User who is sharing
    pub user_id: Uuid,
    /// Username for display
    pub username: String,
    /// Label of shared source (e.g., "Display 1", "Firefox")
    pub source_label: String,
    /// Whether screen audio is included
    pub has_audio: bool,
    /// Current quality tier
    pub quality: Quality,
}

impl ScreenShareInfo {
    /// Create a new ScreenShareInfo.
    pub fn new(
        user_id: Uuid,
        username: String,
        source_label: String,
        has_audio: bool,
        quality: Quality,
    ) -> Self {
        Self {
            user_id,
            username,
            source_label,
            has_audio,
            quality,
        }
    }
}

/// Request to start a screen share.
#[derive(Clone, Debug, Deserialize)]
pub struct ScreenShareStartRequest {
    /// Requested quality tier
    pub quality: Quality,
    /// Include system audio
    pub has_audio: bool,
    /// Source label for display
    pub source_label: String,
}

/// Response to screen share check/start request.
#[derive(Clone, Debug, Serialize)]
pub struct ScreenShareCheckResponse {
    /// Whether screen sharing is allowed
    pub allowed: bool,
    /// Quality tier granted (may be lower than requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_quality: Option<Quality>,
    /// Error message if not allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ScreenShareError>,
}

/// Screen share error reasons.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenShareError {
    /// User doesn't have SCREEN_SHARE permission
    NoPermission,
    /// Channel screen share limit reached
    LimitReached,
    /// User not in the voice channel
    NotInChannel,
    /// Premium quality requested but user lacks PREMIUM_VIDEO
    QualityNotAllowed,
    /// WebRTC renegotiation failed
    RenegotiationFailed,
}

impl ScreenShareCheckResponse {
    /// Create an allowed response.
    pub fn allowed(quality: Quality) -> Self {
        Self {
            allowed: true,
            granted_quality: Some(quality),
            error: None,
        }
    }

    /// Create a denied response.
    pub fn denied(error: ScreenShareError) -> Self {
        Self {
            allowed: false,
            granted_quality: None,
            error: Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_share_info_creation() {
        let user_id = Uuid::new_v4();
        let info = ScreenShareInfo::new(
            user_id,
            "alice".to_string(),
            "Display 1".to_string(),
            true,
            Quality::High,
        );

        assert_eq!(info.user_id, user_id);
        assert_eq!(info.username, "alice");
        assert_eq!(info.source_label, "Display 1");
        assert!(info.has_audio);
        assert_eq!(info.quality, Quality::High);
    }

    #[test]
    fn test_check_response_allowed() {
        let resp = ScreenShareCheckResponse::allowed(Quality::High);
        assert!(resp.allowed);
        assert_eq!(resp.granted_quality, Some(Quality::High));
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_check_response_denied() {
        let resp = ScreenShareCheckResponse::denied(ScreenShareError::LimitReached);
        assert!(!resp.allowed);
        assert!(resp.granted_quality.is_none());
        assert_eq!(resp.error, Some(ScreenShareError::LimitReached));
    }
}
```

**Step 2: Export from voice module**

Add to `server/src/voice/mod.rs`:

```rust
mod screen_share;
pub use screen_share::{
    ScreenShareCheckResponse, ScreenShareError, ScreenShareInfo, ScreenShareStartRequest,
};
```

**Step 3: Run tests**

```bash
SQLX_OFFLINE=true cargo test screen_share --workspace -- --nocapture
```

Expected: 3 tests passing

**Step 4: Commit**

```bash
git add server/src/voice/screen_share.rs server/src/voice/mod.rs
git commit -m "feat(voice): add ScreenShareInfo and related types"
```

---

## Task 6: Extend VoiceParticipant with Screen Sharing Flag

**Files:**
- Modify: `server/src/voice/mod.rs` or `server/src/voice/types.rs` (wherever VoiceParticipant is defined)

**Step 1: Find VoiceParticipant definition**

```bash
grep -r "struct VoiceParticipant" server/src/
```

**Step 2: Add screen_sharing field**

Add to the `VoiceParticipant` struct:

```rust
/// Whether this participant is currently screen sharing
#[serde(default)]
pub screen_sharing: bool,
```

**Step 3: Update any constructors or factory methods**

If there's a `new()` or builder pattern, add the field with default `false`.

**Step 4: Run tests**

```bash
SQLX_OFFLINE=true cargo test voice --workspace -- --nocapture
```

Expected: All voice tests pass

**Step 5: Commit**

```bash
git add server/src/voice/
git commit -m "feat(voice): add screen_sharing flag to VoiceParticipant"
```

---

## Task 7: Register Video Codecs in MediaEngine

**Files:**
- Modify: `server/src/voice/sfu.rs`

**Step 1: Find MediaEngine configuration**

Look for where Opus codec is registered (around line 194-209 based on earlier analysis).

**Step 2: Add VP8 and VP9 video codecs**

After the Opus registration, add:

```rust
// Register VP9 video codec (preferred)
let vp9_codec = RTCRtpCodecCapability {
    mime_type: "video/VP9".to_string(),
    clock_rate: 90000,
    channels: 0,
    sdp_fmtp_line: "profile-id=0".to_string(),
    rtcp_feedback: vec![
        RTCPFeedback {
            typ: "goog-remb".to_string(),
            parameter: "".to_string(),
        },
        RTCPFeedback {
            typ: "ccm".to_string(),
            parameter: "fir".to_string(),
        },
        RTCPFeedback {
            typ: "nack".to_string(),
            parameter: "".to_string(),
        },
        RTCPFeedback {
            typ: "nack".to_string(),
            parameter: "pli".to_string(),
        },
    ],
};
media_engine.register_codec(
    RTCRtpCodecParameters {
        capability: vp9_codec.clone(),
        payload_type: 98,
        ..Default::default()
    },
    RTPCodecType::Video,
)?;

// Register VP8 video codec (fallback)
let vp8_codec = RTCRtpCodecCapability {
    mime_type: "video/VP8".to_string(),
    clock_rate: 90000,
    channels: 0,
    sdp_fmtp_line: "".to_string(),
    rtcp_feedback: vec![
        RTCPFeedback {
            typ: "goog-remb".to_string(),
            parameter: "".to_string(),
        },
        RTCPFeedback {
            typ: "ccm".to_string(),
            parameter: "fir".to_string(),
        },
        RTCPFeedback {
            typ: "nack".to_string(),
            parameter: "".to_string(),
        },
        RTCPFeedback {
            typ: "nack".to_string(),
            parameter: "pli".to_string(),
        },
    ],
};
media_engine.register_codec(
    RTCRtpCodecParameters {
        capability: vp8_codec.clone(),
        payload_type: 96,
        ..Default::default()
    },
    RTPCodecType::Video,
)?;

// Register H.264 video codec (for desktop clients with hardware encoding)
let h264_codec = RTCRtpCodecCapability {
    mime_type: "video/H264".to_string(),
    clock_rate: 90000,
    channels: 0,
    sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".to_string(),
    rtcp_feedback: vec![
        RTCPFeedback {
            typ: "goog-remb".to_string(),
            parameter: "".to_string(),
        },
        RTCPFeedback {
            typ: "ccm".to_string(),
            parameter: "fir".to_string(),
        },
        RTCPFeedback {
            typ: "nack".to_string(),
            parameter: "".to_string(),
        },
        RTCPFeedback {
            typ: "nack".to_string(),
            parameter: "pli".to_string(),
        },
    ],
};
media_engine.register_codec(
    RTCRtpCodecParameters {
        capability: h264_codec.clone(),
        payload_type: 102,
        ..Default::default()
    },
    RTPCodecType::Video,
)?;
```

**Step 3: Add necessary imports**

Ensure these are imported:

```rust
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::rtp_transceiver::RTCPFeedback;
```

**Step 4: Build to verify**

```bash
SQLX_OFFLINE=true cargo build -p vc-server
```

Expected: Build succeeds

**Step 5: Commit**

```bash
git add server/src/voice/sfu.rs
git commit -m "feat(voice): register VP9, VP8, and H.264 video codecs in MediaEngine"
```

---

## Task 8: Add Channel Voice Settings

**Files:**
- Create: `server/migrations/YYYYMMDDHHMMSS_add_channel_voice_settings.sql`
- Modify: `server/src/db/channels.rs` (if exists)

**Step 1: Create migration**

```bash
cd /home/detair/GIT/canis/.worktrees/screen-sharing/server
touch migrations/$(date +%Y%m%d%H%M%S)_add_channel_voice_settings.sql
```

Content:

```sql
-- Add voice-specific settings to channels table
-- These settings apply to voice channels only

ALTER TABLE channels ADD COLUMN max_screen_shares INTEGER NOT NULL DEFAULT 1;

-- Add comment explaining the setting
COMMENT ON COLUMN channels.max_screen_shares IS 'Maximum concurrent screen shares in this channel (default 1)';
```

**Step 2: Update Channel struct**

Add to the Channel struct:

```rust
/// Maximum concurrent screen shares (voice channels only)
#[serde(default = "default_max_screen_shares")]
pub max_screen_shares: i32,
```

Add the default function:

```rust
fn default_max_screen_shares() -> i32 {
    1
}
```

**Step 3: Update channel queries**

Add `max_screen_shares` to SELECT statements for channels.

**Step 4: Commit**

```bash
git add server/migrations/ server/src/db/
git commit -m "feat(db): add max_screen_shares setting to channels"
```

---

## Summary Checklist

After completing all tasks, verify:

- [ ] `TrackSource` and `TrackKind` enums exist with tests
- [ ] `SCREEN_SHARE` permission bit (22) added
- [ ] `users.feature_flags` column migration created
- [ ] `UserFeatures` bitflags added
- [ ] `Quality` enum with all tiers and tests
- [ ] `ScreenShareInfo` and related types exist
- [ ] `VoiceParticipant.screen_sharing` field added
- [ ] Video codecs (VP9, VP8, H.264) registered
- [ ] `channels.max_screen_shares` column migration created

**Final build check:**

```bash
SQLX_OFFLINE=true cargo build --workspace
SQLX_OFFLINE=true cargo test --workspace 2>&1 | grep -E "^test result:"
```

Expected: Build passes, unit tests pass (integration tests may still fail without DB).
