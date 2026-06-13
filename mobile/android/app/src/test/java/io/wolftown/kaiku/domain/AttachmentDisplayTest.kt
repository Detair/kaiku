package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Attachment
import io.wolftown.kaiku.ui.channel.humanReadableSize
import io.wolftown.kaiku.ui.channel.isImage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AttachmentDisplayTest {

    private fun attachment(mime: String) = Attachment(
        id = "a",
        filename = "f",
        mimeType = mime,
        size = 1,
        url = "http://x/f",
    )

    @Test
    fun isImage_recognizesImageMimeTypes() {
        assertTrue(attachment("image/png").isImage())
        assertTrue(attachment("image/jpeg").isImage())
        assertTrue(attachment("IMAGE/WEBP").isImage()) // case-insensitive
    }

    @Test
    fun isImage_rejectsNonImages() {
        assertFalse(attachment("application/pdf").isImage())
        assertFalse(attachment("video/mp4").isImage())
        assertFalse(attachment("text/plain").isImage())
    }

    @Test
    fun humanReadableSize_bytes() {
        assertEquals("0 B", humanReadableSize(0))
        assertEquals("512 B", humanReadableSize(512))
        assertEquals("1023 B", humanReadableSize(1023))
    }

    @Test
    fun humanReadableSize_kilobytes() {
        assertEquals("1 KB", humanReadableSize(1024))
        assertEquals("1.5 KB", humanReadableSize(1536))
    }

    @Test
    fun humanReadableSize_megabytes() {
        assertEquals("1 MB", humanReadableSize(1024L * 1024))
        assertEquals("5 MB", humanReadableSize(5L * 1024 * 1024))
        assertEquals("1.5 MB", humanReadableSize(1024L * 1024 * 3 / 2))
    }

    @Test
    fun humanReadableSize_gigabytes_and_negative() {
        assertEquals("2 GB", humanReadableSize(2L * 1024 * 1024 * 1024))
        assertEquals("0 B", humanReadableSize(-5))
    }
}
