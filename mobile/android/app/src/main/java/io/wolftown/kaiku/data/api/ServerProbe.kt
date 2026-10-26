package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.request.request
import io.ktor.client.request.url
import io.ktor.client.statement.bodyAsText
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.isSuccess
import io.wolftown.kaiku.data.KaikuJson
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.contentOrNull
import java.util.logging.Logger
import javax.inject.Inject

/** What [ServerProbe] found at an address. Carries no server text. */
sealed interface ProbeResult {
    /** A Kaiku server; [registrationOpen] is true only for registration policy `open`. */
    data class Ok(val registrationOpen: Boolean) : ProbeResult

    /** Something answered, but it is not a Kaiku server. */
    data object NotKaiku : ProbeResult

    /** Nothing answered (offline, wrong name, timeout, server error). */
    data object Unreachable : ProbeResult
}

/**
 * Checks that an address is a Kaiku server before the app saves it:
 * `GET /health` must answer 2xx, then `GET /api/settings` supplies the
 * registration policy. Requests carry no credentials.
 */
class ServerProbe @Inject constructor(private val client: HttpClient) {

    private companion object {
        private val logger = Logger.getLogger("ServerProbe")
    }

    suspend fun probe(baseUrl: String): ProbeResult {
        val base = baseUrl.trimEnd('/')
        return try {
            val health = statusOf("$base/health")
            when {
                health.isSuccess() -> Unit
                health.value >= 500 -> return ProbeResult.Unreachable
                else -> return ProbeResult.NotKaiku
            }

            val settingsResponse = client.request {
                method = HttpMethod.Get
                url("$base/api/settings")
                skipAuth()
            }
            if (!settingsResponse.status.isSuccess()) {
                return if (settingsResponse.status.value >= 500) ProbeResult.Unreachable else ProbeResult.NotKaiku
            }
            val settings = parseSettings(settingsResponse.bodyAsText()) ?: return ProbeResult.NotKaiku
            // Only the policy "open" shows Register; invite_only, closed and unknown hide it.
            val policy = (settings["registration_policy"] as? JsonPrimitive)?.contentOrNull
            ProbeResult.Ok(registrationOpen = policy == "open")
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.warning("Server probe failed: ${e.javaClass.simpleName}")
            ProbeResult.Unreachable
        }
    }

    private suspend fun statusOf(url: String): HttpStatusCode =
        client.request {
            method = HttpMethod.Get
            url(url)
            skipAuth()
        }.status

    private fun io.ktor.client.request.HttpRequestBuilder.skipAuth() {
        // Never attach the stored session's token to an address that is only being checked.
        attributes.put(KaikuHttpClient.SkipAuthInterceptor, true)
    }

    /** The settings body as an object, or null when it is not JSON object. */
    private fun parseSettings(body: String): JsonObject? = try {
        KaikuJson.parseToJsonElement(body).jsonObject
    } catch (_: Exception) {
        null
    }
}
