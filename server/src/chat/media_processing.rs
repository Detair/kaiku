//! Image processing for file attachments.
//!
//! Generates blurhash placeholders and multi-resolution thumbnail variants
//! during upload for progressive image loading.

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, ImageReader, Limits};
use thiserror::Error;

/// Maximum file size we'll attempt to process (20 MB).
const MAX_PROCESSABLE_SIZE: usize = 20 * 1024 * 1024;

/// Thumbnail max dimension (256px).
const THUMBNAIL_MAX_DIM: u32 = 256;

/// Medium variant max dimension (1024px).
const MEDIUM_MAX_DIM: u32 = 1024;

/// Blurhash component counts (width x height).
const BLURHASH_COMPONENTS_X: u32 = 4;
const BLURHASH_COMPONENTS_Y: u32 = 3;

/// Size to downscale to before computing blurhash (for speed).
const BLURHASH_SAMPLE_SIZE: u32 = 32;

/// Maximum image dimension (width or height) to prevent decompression bombs.
/// An 8192x8192 RGBA image is ~256 MB in memory.
const MAX_IMAGE_DIMENSION: u32 = 8192;

#[derive(Error, Debug)]
pub enum ProcessingError {
    #[error("File too large for processing: {0} bytes")]
    TooLarge(usize),
    #[error("Unsupported image format: {0}")]
    UnsupportedFormat(String),
    #[error("Image decode failed: {0}")]
    DecodeFailed(String),
    #[error("Blurhash encoding failed: {0}")]
    BlurhashFailed(String),
    #[error("Image encoding failed: {0}")]
    EncodeFailed(String),
}

/// Result of processing an image: dimensions, blurhash, and optional resized variants.
pub struct ImageProcessingResult {
    pub width: u32,
    pub height: u32,
    pub blurhash: String,
    /// 256px max dimension thumbnail (None if original is small enough).
    pub thumbnail: Option<ProcessedVariant>,
    /// 1024px max dimension variant (None if original is small enough).
    pub medium: Option<ProcessedVariant>,
}

/// A resized image variant ready for upload.
pub struct ProcessedVariant {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub content_type: String,
}

/// Process an image: extract dimensions, generate blurhash, and create resized variants.
///
/// For animated formats (GIF), only dimensions and blurhash are generated (no resized
/// variants) to preserve animation.
///
/// This function is CPU-bound and should be called inside `spawn_blocking`.
pub fn process_image(
    data: &[u8],
    mime_type: &str,
) -> Result<ImageProcessingResult, ProcessingError> {
    if data.len() > MAX_PROCESSABLE_SIZE {
        return Err(ProcessingError::TooLarge(data.len()));
    }

    let format = mime_to_format(mime_type)?;
    // Note: animated WebP is not detected here (the `image` crate doesn't expose
    // frame count easily). Animated WebP files will get static variants generated.
    let is_animated = matches!(format, ImageFormat::Gif);

    // Use reader API to enforce dimension limits (prevents decompression bombs:
    // a small compressed file can expand to enormous RGBA buffers)
    let mut reader = ImageReader::with_format(Cursor::new(data), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    reader.limits(limits);

    let img = reader
        .decode()
        .map_err(|e| ProcessingError::DecodeFailed(e.to_string()))?;

    let (width, height) = img.dimensions();

    let blurhash = generate_blurhash(&img)?;

    // Skip resized variants for animated images to preserve animation
    let (thumbnail, medium) = if is_animated {
        (None, None)
    } else {
        let thumbnail = generate_variant(&img, THUMBNAIL_MAX_DIM)?;
        let medium = generate_variant(&img, MEDIUM_MAX_DIM)?;
        (thumbnail, medium)
    };

    Ok(ImageProcessingResult {
        width,
        height,
        blurhash,
        thumbnail,
        medium,
    })
}

/// Map MIME type to `image` crate format.
fn mime_to_format(mime_type: &str) -> Result<ImageFormat, ProcessingError> {
    match mime_type {
        "image/png" => Ok(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Ok(ImageFormat::Jpeg),
        "image/gif" => Ok(ImageFormat::Gif),
        "image/webp" => Ok(ImageFormat::WebP),
        other => Err(ProcessingError::UnsupportedFormat(other.to_string())),
    }
}

/// Generate a blurhash from a small downscaled sample of the image.
fn generate_blurhash(img: &DynamicImage) -> Result<String, ProcessingError> {
    // Downscale to a small size for fast hashing
    let sample = img.resize(
        BLURHASH_SAMPLE_SIZE,
        BLURHASH_SAMPLE_SIZE,
        FilterType::Triangle,
    );
    let (w, h) = sample.dimensions();
    let rgba = sample.to_rgba8();

    blurhash::encode(
        BLURHASH_COMPONENTS_X,
        BLURHASH_COMPONENTS_Y,
        w,
        h,
        rgba.as_raw(),
    )
    .map_err(|e| ProcessingError::BlurhashFailed(e.to_string()))
}

/// Generate a resized WebP variant if the image exceeds `max_dim`.
/// Returns `None` if the image is already smaller than `max_dim`.
fn generate_variant(
    img: &DynamicImage,
    max_dim: u32,
) -> Result<Option<ProcessedVariant>, ProcessingError> {
    let (w, h) = img.dimensions();
    if w <= max_dim && h <= max_dim {
        return Ok(None);
    }

    let resized = img.resize(max_dim, max_dim, FilterType::Lanczos3);
    let (rw, rh) = resized.dimensions();

    let mut buf = std::io::Cursor::new(Vec::new());
    resized
        .write_to(&mut buf, ImageFormat::WebP)
        .map_err(|e| ProcessingError::EncodeFailed(e.to_string()))?;

    Ok(Some(ProcessedVariant {
        data: buf.into_inner(),
        width: rw,
        height: rh,
        content_type: "image/webp".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a small solid-color PNG in memory.
    fn create_test_png(width: u32, height: u32) -> Vec<u8> {
        let img = DynamicImage::new_rgba8(width, height);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    /// Create a small GIF in memory.
    fn create_test_gif(width: u32, height: u32) -> Vec<u8> {
        let img = DynamicImage::new_rgba8(width, height);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Gif).unwrap();
        buf.into_inner()
    }

    #[test]
    fn test_process_small_image_no_variants() {
        let data = create_test_png(100, 100);
        let result = process_image(&data, "image/png").unwrap();

        assert_eq!(result.width, 100);
        assert_eq!(result.height, 100);
        assert!(!result.blurhash.is_empty());
        assert!(
            result.thumbnail.is_none(),
            "100px image should not have a thumbnail"
        );
        assert!(
            result.medium.is_none(),
            "100px image should not have a medium variant"
        );
    }

    #[test]
    fn test_process_large_image_generates_variants() {
        let data = create_test_png(2000, 1500);
        let result = process_image(&data, "image/png").unwrap();

        assert_eq!(result.width, 2000);
        assert_eq!(result.height, 1500);
        assert!(!result.blurhash.is_empty());

        let thumb = result.thumbnail.expect("should have thumbnail");
        assert!(thumb.width <= THUMBNAIL_MAX_DIM);
        assert!(thumb.height <= THUMBNAIL_MAX_DIM);
        assert_eq!(thumb.content_type, "image/webp");
        assert!(!thumb.data.is_empty());

        let medium = result.medium.expect("should have medium variant");
        assert!(medium.width <= MEDIUM_MAX_DIM);
        assert!(medium.height <= MEDIUM_MAX_DIM);
        assert_eq!(medium.content_type, "image/webp");
        assert!(!medium.data.is_empty());
    }

    #[test]
    fn test_process_gif_no_variants() {
        let data = create_test_gif(500, 500);
        let result = process_image(&data, "image/gif").unwrap();

        assert_eq!(result.width, 500);
        assert_eq!(result.height, 500);
        assert!(!result.blurhash.is_empty());
        assert!(result.thumbnail.is_none(), "GIF should not have thumbnail");
        assert!(
            result.medium.is_none(),
            "GIF should not have medium variant"
        );
    }

    #[test]
    fn test_too_large_file_rejected() {
        let err = process_image(&vec![0u8; MAX_PROCESSABLE_SIZE + 1], "image/png");
        assert!(matches!(err, Err(ProcessingError::TooLarge(_))));
    }

    #[test]
    fn test_unsupported_format_rejected() {
        let err = process_image(b"fake", "image/bmp");
        assert!(matches!(err, Err(ProcessingError::UnsupportedFormat(_))));
    }

    #[test]
    fn test_medium_only_for_mid_size_image() {
        // Image bigger than thumbnail but smaller than medium
        let data = create_test_png(800, 600);
        let result = process_image(&data, "image/png").unwrap();

        assert!(
            result.thumbnail.is_some(),
            "800px image should have a thumbnail"
        );
        assert!(
            result.medium.is_none(),
            "800px image should not have a medium variant"
        );
    }
}
