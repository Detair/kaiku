package io.wolftown.kaiku.data.api

import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

/**
 * A request that must reach the server once (the signed claim and the other signed answers) is
 * sent once on the wire, also when the HTTP stack would send it again by itself after a 408 or a
 * 503 that says "again now". Counted at a local test server, on an OkHttp client as the app's.
 */
class OneShotRequestsTest {

    private val server = MockWebServer().apply { start() }

    @After fun tearDown() = server.shutdown()

    private val client = OkHttpClient.Builder()
        .retryOnConnectionFailure(true)
        .addNetworkInterceptor(OneShotRequests())
        .build()

    private fun post(once: Boolean): Request = Request.Builder()
        .url(server.url("/auth/enrollments/claim"))
        .post("{}".toRequestBody("application/json".toMediaType()))
        .apply { if (once) header(OneShotRequests.HEADER, "mark-" + System.nanoTime()) }
        .build()

    private fun sendCounting(once: Boolean, first: MockResponse): Int {
        server.enqueue(first)
        server.enqueue(MockResponse().setResponseCode(201).setBody("{}"))
        val before = server.requestCount
        try {
            client.newCall(post(once)).execute().close()
        } catch (_: IOException) {
            // The repeat was stopped: the answer to the one request that went out is "not known".
        }
        return server.requestCount - before
    }

    @Test fun `a request sent once is not sent again after a 503 that says again now`() {
        assertEquals(1, sendCounting(once = true, MockResponse().setResponseCode(503).setHeader("Retry-After", "0")))
    }

    @Test fun `a request sent once is not sent again after a 408`() {
        assertEquals(1, sendCounting(once = true, MockResponse().setResponseCode(408)))
    }

    @Test fun `without the mark the stack does send it again, which is why the mark exists`() {
        assertEquals(2, sendCounting(once = false, MockResponse().setResponseCode(503).setHeader("Retry-After", "0")))
    }

    @Test fun `the mark itself never reaches the server`() {
        server.enqueue(MockResponse().setResponseCode(201).setBody("{}"))
        client.newCall(post(once = true)).execute().close()
        assertNull(server.takeRequest().getHeader(OneShotRequests.HEADER))
    }

    /** The app's client has it, and the single-use requests of a setup and a sign-in carry the mark. */
    @Test fun `the app's client stops repeats and the single-use requests are marked`() {
        val root = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "src/main/java/io/wolftown/kaiku") }
            .first { it.isDirectory }
        assertTrue(java.io.File(root, "data/api/KaikuHttpClient.kt").readText().contains("addNetworkInterceptor(OneShotRequests())"))
        val enrollment = java.io.File(root, "data/api/EnrollmentApi.kt").readText()
        assertTrue(enrollment.substringAfter("/auth/enrollments/claim").substringBefore("setBody").contains("sentOnce()"))
        val approval = java.io.File(root, "data/api/ApprovalApi.kt").readText()
        assertTrue(approval.substringAfter("\"/auth/device-login\")").substringBefore("setBody").contains("sentOnce()"))
        assertTrue(approval.substringAfter("/answer\")").substringBefore("setBody").contains("sentOnce()"))
        val removal = java.io.File(root, "data/api/RemovalApi.kt").readText()
        assertTrue(removal.substringAfter("/answer\")").substringBefore("setBody").contains("sentOnce()"))
        val auth = java.io.File(root, "data/api/AuthApi.kt").readText()
        assertTrue(auth.substringAfter("/auth/register\")").substringBefore("setBody").contains("sentOnce()"))
    }
}
