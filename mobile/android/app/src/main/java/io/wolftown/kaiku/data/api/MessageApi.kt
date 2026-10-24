package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.Message
import kotlinx.serialization.Serializable
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
}
