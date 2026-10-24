package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

class ServerProbeTest {

    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    private fun probe(handler: (String) -> Pair<HttpStatusCode, String>): Pair<ServerProbe, MutableList<String>> {
        val paths = mutableListOf<String>()
        val engine = MockEngine { request ->
            paths += request.url.encodedPath
            val (status, body) = handler(request.url.encodedPath)
            respond(body, status, json)
        }
        return ServerProbe(HttpClient(engine)) to paths
    }

    @Test
    fun `closed registration is reported as not open`() = runTest {
        val (probe, paths) = probe { path ->
            if (path == "/health") HttpStatusCode.OK to "ok"
            else HttpStatusCode.OK to """{"registration_policy":"closed"}"""
        }

        assertEquals(ProbeResult.Ok(registrationOpen = false), probe.probe("https://k.example"))
        assertEquals(listOf("/health", "/api/settings"), paths)
    }

    @Test
    fun `open registration is reported as open`() = runTest {
        val (probe, _) = probe { path ->
            if (path == "/health") HttpStatusCode.OK to "ok"
            else HttpStatusCode.OK to """{"registration_policy":"open","oidc_enabled":false}"""
        }

        assertEquals(ProbeResult.Ok(registrationOpen = true), probe.probe("https://k.example/"))
    }

    @Test
    fun `invite only and unknown policies are not open`() = runTest {
        for (policy in listOf("invite_only", "weird")) {
            val (probe, _) = probe { path ->
                if (path == "/health") HttpStatusCode.OK to "ok"
                else HttpStatusCode.OK to """{"registration_policy":"$policy"}"""
            }
            assertEquals(ProbeResult.Ok(registrationOpen = false), probe.probe("https://k.example"))
        }
    }

    @Test
    fun `404 is not a Kaiku server`() = runTest {
        val (probe, _) = probe { HttpStatusCode.NotFound to "nope" }

        assertEquals(ProbeResult.NotKaiku, probe.probe("https://example.com"))
    }

    @Test
    fun `health ok but settings missing is not a Kaiku server`() = runTest {
        val (probe, _) = probe { path ->
            if (path == "/health") HttpStatusCode.OK to "ok" else HttpStatusCode.NotFound to ""
        }

        assertEquals(ProbeResult.NotKaiku, probe.probe("https://example.com"))
    }

    @Test
    fun `settings that are not json is not a Kaiku server`() = runTest {
        val (probe, _) = probe { path ->
            if (path == "/health") HttpStatusCode.OK to "ok" else HttpStatusCode.OK to "<html>"
        }

        assertEquals(ProbeResult.NotKaiku, probe.probe("https://example.com"))
    }

    @Test
    fun `server error is unreachable`() = runTest {
        val (probe, _) = probe { HttpStatusCode.BadGateway to "" }

        assertEquals(ProbeResult.Unreachable, probe.probe("https://k.example"))
    }

    @Test
    fun `IOException is unreachable`() = runTest {
        val engine = MockEngine { throw IOException("no route to host") }

        assertEquals(ProbeResult.Unreachable, ServerProbe(HttpClient(engine)).probe("https://k.example"))
    }

    @Test
    fun `probe sends no credentials`() = runTest {
        var authHeader: String? = "unset"
        val engine = MockEngine { request ->
            authHeader = request.headers[HttpHeaders.Authorization]
            respond("ok", HttpStatusCode.NotFound)
        }
        ServerProbe(HttpClient(engine)).probe("https://k.example")

        assertTrue(authHeader == null)
    }
}
