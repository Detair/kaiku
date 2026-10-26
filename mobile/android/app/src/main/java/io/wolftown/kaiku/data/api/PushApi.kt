package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.request.delete
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.http.encodeURLPathPart
import io.ktor.http.isSuccess
import kotlinx.serialization.Serializable
import javax.inject.Inject

/** Transports the server offers (C18, `GET /api/me/push-subscriptions/config`). */
@Serializable
data class PushConfig(
    val providers: List<String> = emptyList(),
    val fcm: FcmPublicConfig? = null,
)

/** The server's Firebase project; FCM works only when the APK was built for the same one. */
@Serializable
data class FcmPublicConfig(val projectId: String = "")

/**
 * Registration of this installation (C19). [deviceId] is a random
 * per-install id ([io.wolftown.kaiku.push.DeviceIdStore]), never the E2EE
 * device id. [toString] hides [endpoint]: an FCM token or UnifiedPush URL is
 * a secret.
 */
@Serializable
data class PushRegistration(
    val provider: String,
    val endpoint: String,
    val deviceId: String,
    val deviceLabel: String? = null,
) {
    override fun toString(): String = "PushRegistration(provider=$provider)"
}

interface PushApi {
    /** `GET /api/me/push-subscriptions/config`. */
    suspend fun getConfig(): PushConfig

    /** `POST /api/me/push-subscriptions` (upsert by device id). */
    suspend fun register(r: PushRegistration)

    /** `DELETE /api/me/push-subscriptions/device/{deviceId}`. */
    suspend fun unregisterDevice(deviceId: String)
}

/**
 * Errors surface as [ApiException] with the HTTP status and server code only;
 * the request body (with the endpoint) never goes into a message.
 */
class PushApiImpl @Inject constructor(
    private val httpClient: HttpClient,
) : PushApi {

    private companion object {
        const val BASE = "/api/me/push-subscriptions"
    }

    override suspend fun getConfig(): PushConfig {
        val response = httpClient.get("$BASE/config")
        response.throwUnlessSuccess("Failed to load push configuration")
        return response.body()
    }

    override suspend fun register(r: PushRegistration) {
        val response = httpClient.post(BASE) { setBody(r) }
        response.throwUnlessSuccess("Failed to register for push")
    }

    override suspend fun unregisterDevice(deviceId: String) {
        val response = httpClient.delete("$BASE/device/${deviceId.encodeURLPathPart()}")
        response.throwUnlessSuccess("Failed to unregister push device")
    }

    private suspend fun HttpResponse.throwUnlessSuccess(fallback: String) {
        if (status.isSuccess()) return
        val errorBody = runCatching { body<ApiErrorResponse>() }.getOrNull()
        throw ApiException(status, fallback, errorBody?.error)
    }
}
