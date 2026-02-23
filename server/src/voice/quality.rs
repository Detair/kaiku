//! Video quality tiers for screen sharing.
//!
//! Provides a `Quality` enum to define video quality presets with
//! resolution, frame rate, and bitrate constraints for screen sharing.

use serde::{Deserialize, Serialize};

/// Video quality tier for screen sharing.
///
/// Each tier defines resolution, frame rate, and bitrate constraints
/// suitable for different network conditions and use cases.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default, utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// 480p @ 15fps, 0.5-1 Mbps - fallback for poor connections.
    Low,
    /// 720p @ 30fps, 1.5-3 Mbps - default quality.
    #[default]
    Medium,
    /// 1080p @ 30fps, 3-5 Mbps - good connections.
    High,
    /// 1080p @ 60fps, 4-8 Mbps - requires `PREMIUM_VIDEO` permission.
    Premium,
}

impl Quality {
    /// Maximum video width in pixels for this quality tier.
    #[must_use]
    pub const fn max_width(&self) -> u32 {
        match self {
            Self::Low => 854,
            Self::Medium => 1280,
            Self::High | Self::Premium => 1920,
        }
    }

    /// Maximum video height in pixels for this quality tier.
    #[must_use]
    pub const fn max_height(&self) -> u32 {
        match self {
            Self::Low => 480,
            Self::Medium => 720,
            Self::High | Self::Premium => 1080,
        }
    }

    /// Maximum frames per second for this quality tier.
    #[must_use]
    pub const fn max_fps(&self) -> u32 {
        match self {
            Self::Low => 15,
            Self::Medium | Self::High => 30,
            Self::Premium => 60,
        }
    }

    /// Target bitrate in bits per second for this quality tier.
    ///
    /// This is the bitrate the encoder should aim for under normal conditions.
    #[must_use]
    pub const fn target_bitrate(&self) -> u32 {
        match self {
            Self::Low => 750_000,       // 750 kbps
            Self::Medium => 2_000_000,  // 2 Mbps
            Self::High => 4_000_000,    // 4 Mbps
            Self::Premium => 6_000_000, // 6 Mbps
        }
    }

    /// Maximum bitrate in bits per second for this quality tier.
    ///
    /// This is the hard upper limit the encoder should never exceed.
    #[must_use]
    pub const fn max_bitrate(&self) -> u32 {
        match self {
            Self::Low => 1_000_000,     // 1 Mbps
            Self::Medium => 3_000_000,  // 3 Mbps
            Self::High => 5_000_000,    // 5 Mbps
            Self::Premium => 8_000_000, // 8 Mbps
        }
    }

    /// Returns true if this quality tier requires premium video permission.
    #[must_use]
    pub const fn requires_premium(&self) -> bool {
        matches!(self, Self::Premium)
    }

    /// Returns the next lower quality tier, or the same tier if already at lowest.
    ///
    /// Useful for adaptive quality when network conditions degrade.
    #[must_use]
    pub const fn downgrade(&self) -> Self {
        match self {
            Self::Premium => Self::High,
            Self::High => Self::Medium,
            Self::Medium | Self::Low => Self::Low,
        }
    }

    /// Returns the next higher quality tier, up to the specified maximum.
    ///
    /// Useful for adaptive quality when network conditions improve.
    ///
    /// # Arguments
    ///
    /// * `max` - The maximum quality tier to upgrade to (e.g., based on permissions).
    #[must_use]
    pub const fn upgrade(&self, max: Self) -> Self {
        // First, determine what the next tier up would be
        let next = match self {
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High | Self::Premium => Self::Premium,
        };

        // Then clamp to the maximum allowed
        // We need to compare enum discriminants since we can't use Ord in const
        let next_ord = next.ordinal();
        let max_ord = max.ordinal();

        if next_ord <= max_ord {
            next
        } else {
            max
        }
    }

    /// Returns the ordinal value of the quality tier (for internal comparison).
    const fn ordinal(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Premium => 3,
        }
    }

    /// Returns all quality tiers in ascending order.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Low, Self::Medium, Self::High, Self::Premium]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_default_is_medium() {
        assert_eq!(Quality::default(), Quality::Medium);
    }

    #[test]
    fn quality_resolution_values() {
        // Low: 854x480
        assert_eq!(Quality::Low.max_width(), 854);
        assert_eq!(Quality::Low.max_height(), 480);

        // Medium: 1280x720
        assert_eq!(Quality::Medium.max_width(), 1280);
        assert_eq!(Quality::Medium.max_height(), 720);

        // High: 1920x1080
        assert_eq!(Quality::High.max_width(), 1920);
        assert_eq!(Quality::High.max_height(), 1080);

        // Premium: 1920x1080
        assert_eq!(Quality::Premium.max_width(), 1920);
        assert_eq!(Quality::Premium.max_height(), 1080);
    }

    #[test]
    fn quality_fps_values() {
        assert_eq!(Quality::Low.max_fps(), 15);
        assert_eq!(Quality::Medium.max_fps(), 30);
        assert_eq!(Quality::High.max_fps(), 30);
        assert_eq!(Quality::Premium.max_fps(), 60);
    }

    #[test]
    fn quality_bitrate_values() {
        // Target bitrates
        assert_eq!(Quality::Low.target_bitrate(), 750_000);
        assert_eq!(Quality::Medium.target_bitrate(), 2_000_000);
        assert_eq!(Quality::High.target_bitrate(), 4_000_000);
        assert_eq!(Quality::Premium.target_bitrate(), 6_000_000);

        // Max bitrates
        assert_eq!(Quality::Low.max_bitrate(), 1_000_000);
        assert_eq!(Quality::Medium.max_bitrate(), 3_000_000);
        assert_eq!(Quality::High.max_bitrate(), 5_000_000);
        assert_eq!(Quality::Premium.max_bitrate(), 8_000_000);

        // Target should always be less than max
        for quality in Quality::all() {
            assert!(
                quality.target_bitrate() < quality.max_bitrate(),
                "Target bitrate should be less than max for {quality:?}"
            );
        }
    }

    #[test]
    fn quality_requires_premium() {
        assert!(!Quality::Low.requires_premium());
        assert!(!Quality::Medium.requires_premium());
        assert!(!Quality::High.requires_premium());
        assert!(Quality::Premium.requires_premium());
    }

    #[test]
    fn quality_downgrade() {
        // Low stays at Low (can't go lower)
        assert_eq!(Quality::Low.downgrade(), Quality::Low);

        // Others step down one tier
        assert_eq!(Quality::Medium.downgrade(), Quality::Low);
        assert_eq!(Quality::High.downgrade(), Quality::Medium);
        assert_eq!(Quality::Premium.downgrade(), Quality::High);
    }

    #[test]
    fn quality_upgrade_without_constraint() {
        // Upgrade with Premium max (no constraint)
        assert_eq!(Quality::Low.upgrade(Quality::Premium), Quality::Medium);
        assert_eq!(Quality::Medium.upgrade(Quality::Premium), Quality::High);
        assert_eq!(Quality::High.upgrade(Quality::Premium), Quality::Premium);
        assert_eq!(Quality::Premium.upgrade(Quality::Premium), Quality::Premium);
    }

    #[test]
    fn quality_upgrade_with_constraint() {
        // Upgrade with High max (no premium allowed)
        assert_eq!(Quality::Low.upgrade(Quality::High), Quality::Medium);
        assert_eq!(Quality::Medium.upgrade(Quality::High), Quality::High);
        assert_eq!(Quality::High.upgrade(Quality::High), Quality::High);

        // Upgrade with Medium max
        assert_eq!(Quality::Low.upgrade(Quality::Medium), Quality::Medium);
        assert_eq!(Quality::Medium.upgrade(Quality::Medium), Quality::Medium);

        // Upgrade with Low max (stay at Low)
        assert_eq!(Quality::Low.upgrade(Quality::Low), Quality::Low);
    }

    #[test]
    fn quality_serialization() {
        // Serialize to JSON
        let json = serde_json::to_string(&Quality::Medium).unwrap();
        assert_eq!(json, "\"medium\"");

        let json = serde_json::to_string(&Quality::Premium).unwrap();
        assert_eq!(json, "\"premium\"");

        // Deserialize from JSON
        let quality: Quality = serde_json::from_str("\"low\"").unwrap();
        assert_eq!(quality, Quality::Low);

        let quality: Quality = serde_json::from_str("\"high\"").unwrap();
        assert_eq!(quality, Quality::High);
    }

    #[test]
    fn quality_all_tiers_ordered() {
        let all = Quality::all();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0], Quality::Low);
        assert_eq!(all[1], Quality::Medium);
        assert_eq!(all[2], Quality::High);
        assert_eq!(all[3], Quality::Premium);
    }
}
