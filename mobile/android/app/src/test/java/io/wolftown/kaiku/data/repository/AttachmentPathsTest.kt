package io.wolftown.kaiku.data.repository

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.nio.file.Files

/** AND-B-4: a server-sent file name can never write outside the download cache. */
class AttachmentPathsTest {

    private val base: File = Files.createTempDirectory("att").toFile()

    @Test
    fun `path parts and leading dots are removed`() {
        assertEquals("passwd", AttachmentPaths.safeName("../../etc/passwd"))
        assertEquals("evil.txt", AttachmentPaths.safeName("..\\\\..\\\\evil.txt"))
        assertEquals("hidden", AttachmentPaths.safeName(".hidden"))
        assertEquals("file", AttachmentPaths.safeName(".."))
    }

    @Test
    fun `ordinary names stay readable`() {
        assertEquals("Urlaub 2026.jpg", AttachmentPaths.safeName("Urlaub 2026.jpg"))
        assertEquals("Größe_1_.pdf", AttachmentPaths.safeName("Größe(1).pdf"))
    }

    @Test
    fun `long names keep their extension`() {
        val name = AttachmentPaths.safeName("a".repeat(300) + ".mp4")
        assertEquals(100, name.length)
        assertTrue(name.endsWith(".mp4"))
    }

    @Test
    fun `cache file stays inside the base folder`() {
        val file = AttachmentPaths.cacheFile(base, "../../x", "../../../y.jpg")
        assertTrue(file.canonicalPath.startsWith(base.canonicalPath + File.separator))
        assertFalse(file.canonicalPath.contains(".."))
    }

    @Test
    fun `download limit allows slack and caps unknown sizes`() {
        assertEquals(10L + 1024 * 1024, AttachmentPaths.downloadLimit(10))
        assertEquals(AttachmentPaths.UNKNOWN_SIZE_LIMIT, AttachmentPaths.downloadLimit(0))
    }
}
