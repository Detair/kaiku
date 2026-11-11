package io.wolftown.kaiku.data.repository

import android.content.Context
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.Attachment
import kotlinx.coroutines.test.runTest
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody
import okhttp3.ResponseBody.Companion.toResponseBody
import okio.BufferedSource
import okio.buffer
import okio.source
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.nio.file.Files

/** AND-B-4 downloads: bearer only for the own server, size cap, no half files, cleared on logout. */
class AttachmentFilesTest {

    private val cache: File = Files.createTempDirectory("kaiku-cache").toFile()
    private val context = mockk<Context> { every { cacheDir } returns cache }
    private val tokens = mockk<TokenStorage> {
        every { getServerUrl() } returns "https://kaiku.test"
        every { getAccessToken() } returns "secret-token"
    }
    private val seen = mutableListOf<Request>()

    private fun files(body: () -> ResponseBody): AttachmentFiles {
        val transport = Interceptor { chain ->
            seen += chain.request()
            Response.Builder()
                .request(chain.request())
                .protocol(Protocol.HTTP_1_1)
                .code(200)
                .message("OK")
                .body(body())
                .build()
        }
        return AttachmentFiles(context, OkHttpClient(), tokens, transport)
    }

    private fun attachment(url: String, size: Long, id: String = "a1") =
        Attachment(id = id, filename = "photo.jpg", mimeType = "image/jpeg", size = size, url = url)

    private fun partFiles() = cache.walkTopDown().filter { it.name.endsWith(".part") }.toList()

    @Test
    fun `own server gets the token and the file lands in the cache`() = runTest {
        val bytes = ByteArray(10) { it.toByte() }
        val file = files { bytes.toResponseBody("image/jpeg".toMediaType()) }
            .download(attachment("/api/files/a1", size = 10))

        assertEquals("Bearer secret-token", seen.single().header("Authorization"))
        assertArrayEquals(bytes, file.readBytes())
        assertTrue(file.canonicalPath.startsWith(File(cache, "attachments").canonicalPath))
    }

    @Test
    fun `another host never gets the token`() = runTest {
        files { ByteArray(3).toResponseBody(null) }
            .download(attachment("https://files.elsewhere.example/a1", size = 3))

        assertNull(seen.single().header("Authorization"))
    }

    @Test
    fun `a download larger than announced stops and leaves no part file`() = runTest {
        val tooBig = ByteArray(10 + 1024 * 1024 + 1)
        val result = runCatching {
            files { tooBig.toResponseBody(null) }.download(attachment("/api/files/a1", size = 10))
        }

        assertTrue(result.exceptionOrNull() is IOException)
        assertTrue(partFiles().isEmpty())
        assertFalse(File(cache, "attachments/a1/photo.jpg").exists())
    }

    @Test
    fun `a connection that breaks midway leaves no part file`() = runTest {
        val breaking = object : InputStream() {
            var n = 0
            override fun read(): Int = if (n++ < 100) 1 else throw IOException("connection reset")
        }
        val result = runCatching {
            files { breaking.source().buffer().asBody() }.download(attachment("/api/files/a1", size = 1000))
        }

        assertTrue(result.exceptionOrNull() is IOException)
        assertTrue(partFiles().isEmpty())
    }

    @Test
    fun `clearing the cache removes every download`() = runTest {
        val f = files { ByteArray(4).toResponseBody(null) }
        f.download(attachment("/api/files/a1", size = 4))
        assertTrue(File(cache, "attachments").exists())

        f.clearCache()

        assertFalse(File(cache, "attachments").exists())
    }

    private fun BufferedSource.asBody(): ResponseBody {
        val source = this
        return object : ResponseBody() {
            override fun contentType() = null
            override fun contentLength() = -1L
            override fun source(): BufferedSource = source
        }
    }
}
