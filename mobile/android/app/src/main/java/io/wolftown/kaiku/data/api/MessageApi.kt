package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.request.*
import io.ktor.client.request.forms.InputProvider
import io.ktor.client.request.forms.formData
import io.ktor.client.request.forms.submitFormWithBinaryData
import io.ktor.http.*
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.Message
import kotlinx.serialization.Serializable
import io.ktor.utils.io.streams.asInput
import java.io.InputStream
import java.net.URLEncoder
import javax.inject.Inject

interface MessageApi {
    /**
     * Loads one page of channel history (`GET /api/messages/channel/{id}`).
     *
     * Pass at most one cursor: [before] pages backwards from a message id,
     * [after] pages forwards. Passing both throws [IllegalArgumentException].
     * Callers must not rely on the item order of the page; merge it with
     * [io.wolftown.kaiku.domain.MessageOrdering.merge].
     */
    suspend fun getMessages(
        channelId: String,
        before: String? = null,
        after: String? = null,
        limit: Int = 50
    ): CursorPage<Message>
    suspend fun sendMessage(channelId: String, content: String): Message
    suspend fun editMessage(messageId: String, content: String): Message
    suspend fun deleteMessage(messageId: String)
    suspend fun addReaction(channelId: String, messageId: String, emoji: String)
    suspend fun removeReaction(channelId: String, messageId: String, emoji: String)

    /**
     * Sends a message with one attachment (`POST /api/messages/channel/{id}/upload`,
     * multipart fields `file` and `content`). [content] is the optional caption.
     * The file is streamed from [open] (called once per request attempt), never
     * held in memory; [size] is its length in bytes or null if unknown.
     * The caller must have checked the size against [getUploadLimits] already.
     */
    suspend fun uploadMessageWithFile(
        channelId: String,
        fileName: String,
        mimeType: String,
        size: Long?,
        open: () -> InputStream,
        content: String?,
        /** Session the upload belongs to; it is never sent in another session (see [KaikuHttpClient.RequiredSessionId]). */
        sessionId: Long? = null
    ): Message

    /** `GET /api/config/upload-limits`; callers cache it per session. */
    suspend fun getUploadLimits(): UploadLimits
}

/** Upload size limits in bytes; `maxUploadSize` applies to message attachments. */
@Serializable
data class UploadLimits(
    val maxAvatarSize: Long = 0L,
    val maxEmojiSize: Long = 0L,
    val maxUploadSize: Long = DEFAULT_MAX_UPLOAD_SIZE,
) {
    companion object {
        /** Server default (50 MB), used if the server omits the field. */
        const val DEFAULT_MAX_UPLOAD_SIZE = 50L * 1024 * 1024
    }
}

@Serializable
private data class SendMessageRequest(val content: String)

@Serializable
private data class EditMessageRequest(val content: String)

@Serializable
private data class ReactionRequest(val emoji: String)

class MessageApiImpl @Inject constructor(
    private val httpClient: HttpClient
) : MessageApi {

    override suspend fun getMessages(
        channelId: String,
        before: String?,
        after: String?,
        limit: Int
    ): CursorPage<Message> {
        require(before == null || after == null) { "Pass either before or after, not both" }

        val response = httpClient.get("/api/messages/channel/$channelId") {
            parameter("limit", limit)
            if (before != null) {
                parameter("before", before)
            }
            if (after != null) {
                parameter("after", after)
            }
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Failed to load messages"
            )
        }

        return response.body()
    }

    override suspend fun sendMessage(channelId: String, content: String): Message {
        val response = httpClient.post("/api/messages/channel/$channelId") {
            setBody(SendMessageRequest(content))
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Failed to send message"
            )
        }

        return response.body()
    }

    override suspend fun editMessage(messageId: String, content: String): Message {
        val response = httpClient.patch("/api/messages/$messageId") {
            setBody(EditMessageRequest(content))
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Failed to edit message"
            )
        }

        return response.body()
    }

    override suspend fun deleteMessage(messageId: String) {
        val response = httpClient.delete("/api/messages/$messageId")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Failed to delete message"
            )
        }
    }

    override suspend fun addReaction(channelId: String, messageId: String, emoji: String) {
        val response = httpClient.put("/api/channels/$channelId/messages/$messageId/reactions") {
            setBody(ReactionRequest(emoji))
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Failed to add reaction"
            )
        }
    }

    override suspend fun removeReaction(channelId: String, messageId: String, emoji: String) {
        val response = httpClient.delete("/api/channels/$channelId/messages/$messageId/reactions/$emoji")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Failed to remove reaction"
            )
        }
    }

    override suspend fun uploadMessageWithFile(
        channelId: String,
        fileName: String,
        mimeType: String,
        size: Long?,
        open: () -> InputStream,
        content: String?,
        sessionId: Long?
    ): Message {
        val response = httpClient.submitFormWithBinaryData(
            url = "/api/messages/channel/$channelId/upload",
            formData = formData {
                append("file", InputProvider(size) { open().asInput() }, Headers.build {
                    append(HttpHeaders.ContentType, mimeType)
                    append(HttpHeaders.ContentDisposition, filenameDisposition(fileName))
                })
                if (!content.isNullOrBlank()) append("content", content)
            }
        ) {
            if (sessionId != null) attributes.put(KaikuHttpClient.RequiredSessionId, sessionId)
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(
                response.status,
                errorBody?.message ?: "Failed to upload file",
                errorBody?.error
            )
        }

        return response.body()
    }

    override suspend fun getUploadLimits(): UploadLimits {
        val response = httpClient.get("/api/config/upload-limits")

        if (!response.status.isSuccess()) {
            throw ApiException(response.status, "Failed to load upload limits")
        }

        return response.body()
    }
}

/**
 * `filename="<ascii fallback>"` plus, for non-ASCII names, RFC 5987
 * `filename*=UTF-8''<percent-encoded>`. Quotes, backslashes, control
 * characters and line breaks never reach the header.
 */
internal fun filenameDisposition(fileName: String): String {
    val fallback = buildString {
        for (c in fileName) {
            append(if (c.code in 0x20..0x7E && c != '"' && c != '\\') c else '_')
        }
    }.ifEmpty { "upload" }
    val plain = "filename=\"$fallback\""
    if (fileName.all { it.code in 0x20..0x7E && it != '"' && it != '\\' }) return plain
    val cleaned = fileName.filter { it.code >= 0x20 && it.code != 0x7F }
    val encoded = URLEncoder.encode(cleaned, "UTF-8").replace("+", "%20").replace("*", "%2A")
    return "$plain; filename*=UTF-8''$encoded"
}
