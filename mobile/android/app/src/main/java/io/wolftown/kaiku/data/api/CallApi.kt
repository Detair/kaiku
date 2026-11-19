package io.wolftown.kaiku.data.api

import io.ktor.client.HttpClient
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.client.statement.HttpResponse
import io.ktor.client.statement.bodyAsText
import io.ktor.http.isSuccess
import io.wolftown.kaiku.data.KaikuJson
import kotlinx.serialization.Serializable
import javax.inject.Inject

/**
 * A DM call as the server answers `GET /api/dm/{id}/call` and every call
 * action: `status` is `ringing`, `active` or `ended`; the other fields are
 * those of that status (empty otherwise).
 */
@Serializable
data class CallStateDto(
    val channelId: String,
    val callId: String,
    val status: String,
    val startedBy: String? = null,
    val targetUsers: List<String> = emptyList(),
    val declinedBy: List<String> = emptyList(),
    val participants: List<String> = emptyList(),
)

/** The DM call endpoints (`/api/dm/{id}/call[...]`). Errors are [ApiException] with the server's `code`. */
interface CallApi {
    /** The DM's current call, or null when there is none. */
    suspend fun get(channelId: String): CallStateDto?
    suspend fun start(channelId: String): CallStateDto
    suspend fun join(channelId: String): CallStateDto
    suspend fun decline(channelId: String): CallStateDto
    suspend fun leave(channelId: String): CallStateDto

    companion object {
        /** 409: the other side's call already exists (glare); its ring wins. */
        const val CALL_ALREADY_EXISTS = "CALL_ALREADY_EXISTS"

        /** 403: a block between the two accounts. */
        const val BLOCKED = "BLOCKED"

        /** 404: the call is gone. */
        const val CALL_NOT_FOUND = "CALL_NOT_FOUND"
    }
}

/** Call errors put the code in `code` and a text in `error` (unlike the C16 shape). */
@Serializable
private data class CallErrorBody(val error: String? = null, val code: String? = null)

class CallApiImpl @Inject constructor(
    private val httpClient: HttpClient,
) : CallApi {

    override suspend fun get(channelId: String): CallStateDto? {
        val text = httpClient.get("/api/dm/$channelId/call").checked("get call failed").bodyAsText()
        if (text.isBlank() || text.trim() == "null") return null
        return KaikuJson.decodeFromString(CallStateDto.serializer(), text)
    }

    override suspend fun start(channelId: String) = action(channelId, "start")
    override suspend fun join(channelId: String) = action(channelId, "join")
    override suspend fun decline(channelId: String) = action(channelId, "decline")
    override suspend fun leave(channelId: String) = action(channelId, "leave")

    private suspend fun action(channelId: String, what: String): CallStateDto {
        val response = httpClient.post("/api/dm/$channelId/call/$what").checked("call $what failed")
        return KaikuJson.decodeFromString(CallStateDto.serializer(), response.bodyAsText())
    }

    private suspend fun HttpResponse.checked(fallback: String): HttpResponse {
        if (status.isSuccess()) return this
        val body = runCatching {
            KaikuJson.decodeFromString(CallErrorBody.serializer(), bodyAsText())
        }.getOrNull()
        throw ApiException(status, fallback, code = body?.code ?: body?.error)
    }
}
