package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.wolftown.kaiku.domain.model.UnreadAggregate
import javax.inject.Inject

interface UnreadApi {
    /** The server's authoritative unread counts (`GET /api/me/unread`). */
    suspend fun getUnread(): UnreadAggregate
}

class UnreadApiImpl @Inject constructor(
    private val httpClient: HttpClient
) : UnreadApi {

    override suspend fun getUnread(): UnreadAggregate {
        val response = httpClient.get("/api/me/unread")

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to load unread counts", errorBody?.error)
        }

        return response.body()
    }
}
