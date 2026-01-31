//! Shared utility functions

/// Format file size in human-readable units
///
/// # Examples
///
/// ```
/// use vc_server::util::format_file_size;
///
/// assert_eq!(format_file_size(512), "512 bytes");
/// assert_eq!(format_file_size(2048), "2KB");
/// assert_eq!(format_file_size(5 * 1024 * 1024), "5.0MB");
/// ```
pub fn format_file_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else if bytes < 1024 * 1024 {
        format!("{}KB", bytes / 1024)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(0), "0 bytes");
        assert_eq!(format_file_size(512), "512 bytes");
        assert_eq!(format_file_size(1023), "1023 bytes");
        assert_eq!(format_file_size(1024), "1KB");
        assert_eq!(format_file_size(2048), "2KB");
        assert_eq!(format_file_size(256 * 1024), "256KB");
        assert_eq!(format_file_size(1024 * 1024 - 1), "1023KB");
        assert_eq!(format_file_size(1024 * 1024), "1.0MB");
        assert_eq!(format_file_size(5 * 1024 * 1024), "5.0MB");
        assert_eq!(format_file_size(5_500_000), "5.2MB");
    }
}
