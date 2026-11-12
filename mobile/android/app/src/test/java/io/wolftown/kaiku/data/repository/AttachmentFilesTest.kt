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

    /** Real session state; observers run at once (unconfined). */
    private val scope = kotlinx.coroutines.CoroutineScope(
        kotlinx.coroutines.SupervisorJob() + kotlinx.coroutines.Dispatchers.Unconfined
    )
    private val auth = io.wolftown.kaiku.data.local.AuthState(scope).apply { setLoggedIn("anna") }

    /** Runs inside the network call, before the body is read (a race at the worst moment). */
    private var duringCall: (AttachmentFiles) -> Unit = {}
    private lateinit var current: AttachmentFiles

    private fun files(body: () -> ResponseBody): AttachmentFiles {
        val transport = Interceptor { chain ->
            seen += chain.request()
            duringCall(current)
            Response.Builder()
                .request(chain.request())
                .protocol(Protocol.HTTP_1_1)
                .code(200)
                .message("OK")
                .body(body())
                .build()
        }
        current = AttachmentFiles(context, OkHttpClient(), tokens, auth, scope, transport)
        return current
    }

    private fun attachmentsDir() = File(cache, "attachments")

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

    @Test
    fun `clearing the cache during a download leaves nothing of it behind`() = runTest {
        duringCall = { it.clearCache() } // logout teardown while the server is answering
        val result = runCatching {
            files { ByteArray(10).toResponseBody(null) }.download(attachment("/api/files/a1", size = 10))
        }

        assertTrue(result.isFailure) // cancelled call or dropped claim
        assertFalse(attachmentsDir().exists())
        assertTrue(partFiles().isEmpty())
    }

    @Test
    fun `a download paused before its folder exists cannot bring back a cleared cache`() = runTest {
        // Teardown clears the cache after the download started but before it created its folder.
        var cleared = false
        every { tokens.getServerUrl() } answers {
            if (!cleared) {
                cleared = true
                current.clearCache()
            }
            "https://kaiku.test"
        }
        val f = files { ByteArray(10).toResponseBody(null) }

        val result = runCatching { f.download(attachment("/api/files/a1", size = 10)) }

        assertTrue(result.isFailure)
        assertTrue(seen.isEmpty())
        assertFalse(attachmentsDir().exists())
    }

    @Test
    fun `ending the session during a download leaves nothing of it behind`() = runTest {
        duringCall = { auth.setLoggedOut() }
        val result = runCatching {
            files { ByteArray(10).toResponseBody(null) }.download(attachment("/api/files/a1", size = 10))
        }

        assertTrue(result.isFailure)
        assertFalse(attachmentsDir().exists())
    }

    @Test
    fun `a download of account A started before switching to B is never sent`() = runTest {
        // The switch lands after the download took its claim, before the request goes out.
        every { tokens.getServerUrl() } answers {
            if (auth.currentUserId.value == "anna") auth.setLoggedIn("bert")
            "https://kaiku.test"
        }
        val f = files { ByteArray(3).toResponseBody(null) }

        val result = runCatching { f.download(attachment("/api/files/a1", size = 3)) }

        assertTrue(result.isFailure)
        assertTrue(seen.isEmpty()) // never reached the network, so never with B's token
        assertFalse(attachmentsDir().exists())
    }

    @Test
    fun `clearing moves the folder away at once and deletes it in the background`() = runTest {
        val f = files { ByteArray(4).toResponseBody(null) }
        f.download(attachment("/api/files/a1", size = 4))

        f.clearCache()

        assertFalse(attachmentsDir().exists())
        assertTrue(cache.listFiles().orEmpty().none { it.name.startsWith("attachments") })
    }
}
