package io.wolftown.kaiku.testutil

import io.ktor.client.HttpClient
import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.Headers
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage

/** One request as the server saw it. */
data class Recorded(
    val method: HttpMethod,
    val path: String,
    val query: String,
    val body: String,
    val headers: Headers,
    val url: String,
)

/**
 * A configured [KaikuHttpClient] over a MockEngine that records every request
 * and answers with [status] / [body] (or [respond] per request).
 */
class ApiRecorder(
    var status: HttpStatusCode = HttpStatusCode.OK,
    var body: String = "{}",
    var bearer: String? = "stored-access-token",
    extraHeaders: Map<String, String> = emptyMap(),
) {
    val requests = mutableListOf<Recorded>()
    var respond: ((Recorded) -> Pair<HttpStatusCode, String>)? = null
    private val extra = extraHeaders

    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns "https://chat.example.com"
        every { getAccessToken() } answers { bearer }
    }
    private val authState: AuthState = mockk(relaxed = true)

    val client: HttpClient = KaikuHttpClient.forTesting(
        tokenStorage,
        authState,
        MockEngine { request ->
            val rec = Recorded(
                method = request.method,
                path = request.url.encodedPath,
                query = request.url.encodedQuery,
                body = String(request.body.toByteArray()),
                headers = request.headers,
                url = request.url.toString(),
            )
            requests += rec
            val (s, b) = respond?.invoke(rec) ?: (status to body)
            val headerPairs = listOf(HttpHeaders.ContentType to listOf("application/json")) +
                extra.map { it.key to listOf(it.value) }
            respond(b, s, headersOf(*headerPairs.toTypedArray()))
        },
    ).activeClient()

    val last: Recorded get() = requests.last()
}
