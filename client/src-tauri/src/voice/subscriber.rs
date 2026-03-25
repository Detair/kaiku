//! Subscriber Track Routing
//!
//! Parse stream IDs from the SFU and classify tracks by type.

/// Parse a stream ID like `"uuid:microphone"` or `"uuid:screen_video:stream_uuid"`.
///
/// Returns `(user_id, source_type)` where `source_type` is everything after the
/// first colon (e.g. `"microphone"`, `"screen_video:stream_uuid"`).
pub fn parse_stream_id(stream_id: &str) -> (String, String) {
    let colon = stream_id.find(':').unwrap_or(stream_id.len());
    let user_id = stream_id[..colon].to_string();
    let source_type = if colon < stream_id.len() {
        stream_id[colon + 1..].to_string()
    } else {
        String::new()
    };
    (user_id, source_type)
}

/// Returns `true` if the source type represents an audio track.
pub fn is_audio_track(source_type: &str) -> bool {
    source_type == "microphone" || source_type.starts_with("screen_audio")
}

/// Returns `true` if the source type represents a video track.
pub fn is_video_track(source_type: &str) -> bool {
    source_type == "webcam" || source_type.starts_with("screen_video")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stream_id_microphone() {
        let (user, source) = parse_stream_id("abc-123:microphone");
        assert_eq!(user, "abc-123");
        assert_eq!(source, "microphone");
    }

    #[test]
    fn test_parse_stream_id_screen_video_with_stream() {
        let (user, source) = parse_stream_id("abc-123:screen_video:stream-456");
        assert_eq!(user, "abc-123");
        assert_eq!(source, "screen_video:stream-456");
    }

    #[test]
    fn test_parse_stream_id_no_colon() {
        let (user, source) = parse_stream_id("abc-123");
        assert_eq!(user, "abc-123");
        assert_eq!(source, "");
    }

    #[test]
    fn test_is_audio_track() {
        assert!(is_audio_track("microphone"));
        assert!(is_audio_track("screen_audio"));
        assert!(is_audio_track("screen_audio:stream-456"));
        assert!(!is_audio_track("webcam"));
        assert!(!is_audio_track("screen_video"));
        assert!(!is_audio_track(""));
    }

    #[test]
    fn test_is_video_track() {
        assert!(is_video_track("webcam"));
        assert!(is_video_track("screen_video"));
        assert!(is_video_track("screen_video:stream-456"));
        assert!(!is_video_track("microphone"));
        assert!(!is_video_track("screen_audio"));
        assert!(!is_video_track(""));
    }
}
