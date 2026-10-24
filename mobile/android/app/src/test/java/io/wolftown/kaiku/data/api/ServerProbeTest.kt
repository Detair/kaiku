package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

class ServerProbeTest {

    private val json = headersOf(HttpHeaders.ContentType, "application/json")

    private fun serverProbeFor(handler: (String) -> Pair<HttpStatusCode, String>): Pair<ServerProbe, MutableList<String>> {
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
        val (probe, paths) = serverProbeFor { path ->
            if (path == "/health") HttpStatusCode.OK to "ok"
            else HttpStatusCode.OK to """{"registration_policy":"closed"}"""
        }

        assertEquals(ProbeResult.Ok(registrationOpen = false), probe.probe("https://k.example"))
        assertEquals(listOf("/health", "/api/settings"), paths)
    }

    @Test
    fun `open registration is reported as open`() = runTest {
        val (probe, _) = serverProbeFor { path ->
            if (path == "/health") HttpStatusCode.OK to "ok"
            else HttpStatusCode.OK to """{"registration_policy":"open","oidc_enabled":false}"""
        }

        assertEquals(ProbeResult.Ok(registrationOpen = true), probe.probe("https://k.example/"))
    }

    @Test
    fun `invite only and unknown policies are not open`() = runTest {
        for (policy in listOf("invite_only", "weird")) {
            val (probe, _) = serverProbeFor { path ->
                if (path == "/health") HttpStatusCode.OK to "ok"
                else HttpStatusCode.OK to """{"registration_policy":"$policy"}"""
            }
            assertEquals(ProbeResult.Ok(registrationOpen = false), probe.probe("https://k.example"))
        }
    }

    @Test
    fun `404 is not a Kaiku server`() = runTest {
        val (probe, _) = serverProbeFor { HttpStatusCode.NotFound to "nope" }

        assertEquals(ProbeResult.NotKaiku, probe.probe("https://example.com"))
    }

    @Test
    fun `health ok but settings missing is not a Kaiku server`() = runTest {
        val (probe, _) = serverProbeFor { path ->
            if (path == "/health") HttpStatusCode.OK to "ok" else HttpStatusCode.NotFound to ""
        }

        assertEquals(ProbeResult.NotKaiku, probe.probe("https://example.com"))
    }

    @Test
    fun `settings that are not json is not a Kaiku server`() = runTest {
        val (probe, _) = serverProbeFor { path ->
            if (path == "/health") HttpStatusCode.OK to "ok" else HttpStatusCode.OK to "<html>"
        }

        assertEquals(ProbeResult.NotKaiku, probe.probe("https://example.com"))
    }

    @Test
    fun `server error is unreachable`() = runTest {
        val (probe, _) = serverProbeFor { HttpStatusCode.BadGateway to "" }

        assertEquals(ProbeResult.Unreachable, probe.probe("https://k.example"))
    }

    @Test
    fun `IOException is unreachable`() = runTest {
        val engine = MockEngine { throw IOException("no route to host") }

        assertEquals(ProbeResult.Unreachable, ServerProbe(HttpClient(engine)).probe("https://k.example"))
    }

    @Test
    fun `probe sends no credentials and never refreshes`() = runTest {
        val tokenStorage = mockk<TokenStorage>(relaxed = true)
        every { tokenStorage.getServerUrl() } returns "https://stored.example"
        every { tokenStorage.getAccessToken() } returns "stored-access-token"
        every { tokenStorage.getRefreshToken() } returns "stored-refresh-token"
        val authState = mockk<AuthState>(relaxed = true)

        val seen = mutableListOf<Pair<String, String?>>()
        val engine = MockEngine { request ->
            seen += request.url.encodedPath to request.headers[HttpHeaders.Authorization]
            respond("", HttpStatusCode.Unauthorized)
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient()

        val result = ServerProbe(client).probe("https://other.example")

        assertEquals(ProbeResult.NotKaiku, result)
        assertEquals(listOf("/health"), seen.map { it.first })
        assertTrue(seen.all { it.second == null })
        // No refresh request and no session change for an address that is only checked.
        verify(exactly = 0) { tokenStorage.saveTokensIfRefreshTokenIs(any(), any(), any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.clearIfRefreshTokenIs(any()) }
    }
}
