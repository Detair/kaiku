package io.wolftown.kaiku.data.api

import io.ktor.client.call.body
import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.statement.HttpResponse
import io.ktor.http.HttpHeaders
import io.ktor.http.isSuccess

/**
 * Throws an [ApiException] with the server code and its fields for a non-2xx
 * [HttpResponse]. [fallback] is a log text only; user text comes from string
 * resources for the code (C16), never from the server `message`.
 */
internal suspend fun HttpResponse.ensureSuccess(fallback: String) {
    if (status.isSuccess()) return
    val body = try {
        body<ApiErrorResponse>()
    } catch (e: kotlin.coroutines.cancellation.CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }
    val retryAfter = headers[HttpHeaders.RetryAfter]?.trim()?.toLongOrNull()
    throw ApiException(
        status,
        body?.message ?: fallback,
        code = body?.error,
        fields = (body?.fields() ?: ApiErrorFields()).copy(retryAfterSeconds = retryAfter),
    )
}

/**
 * Marks a request to a public route: no stored bearer is attached and a 401
 * never triggers a refresh. Required for routes that may go to another origin
 * than the configured server (setup of a new phone), so a session token never
 * leaves for another server.
 */
internal fun HttpRequestBuilder.publicRoute() {
    attributes.put(KaikuHttpClient.SkipAuthInterceptor, true)
}

/** Absolute URL on [origin], or the configured server's relative path when null. */
internal fun at(origin: String?, path: String): String = origin?.trimEnd('/')?.plus(path) ?: path
