package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.wolftown.kaiku.domain.model.DmConversation
import javax.inject.Inject

interface DmApi {
    /** List the current user's DM conversations, newest activity first. */
    suspend fun getDms(): List<DmConversation>

    /** Mark a DM as read up to its latest message, clearing the unread count. */
    suspend fun markAsRead(channelId: String)
}

class DmApiImpl @Inject constructor(
    private val httpClient: HttpClient
) : DmApi {

    override suspend fun getDms(): List<DmConversation> {
        val response = httpClient.get("/api/dm")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to load direct messages")
        }

        return response.body()
    }

    override suspend fun markAsRead(channelId: String) {
        val response = httpClient.post("/api/dm/$channelId/read")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to mark DM as read")
        }
    }
}
