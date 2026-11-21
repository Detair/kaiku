package io.wolftown.kaiku.data.api

import io.ktor.client.*
import io.ktor.client.call.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.wolftown.kaiku.domain.model.BlockedUser
import io.wolftown.kaiku.domain.model.DmConversation
import kotlinx.serialization.Serializable
import javax.inject.Inject

@Serializable
private data class MarkReadRequest(val lastReadMessageId: String)

@Serializable
private data class CreateDmRequest(val participantIds: List<String>)

/**
 * The DM endpoints. A request made inside [withRequestSession] is bound to
 * that signed-in session and is not sent once another one is current.
 */
interface DmApi {
    /** List the current user's DM conversations, newest activity first. */
    suspend fun getDms(): List<DmConversation>

    /** Mark a DM as read up to its latest message, clearing the unread count. */
    suspend fun markAsRead(channelId: String, lastReadMessageId: String)

    /**
     * Opens the 1:1 DM with [userId]: `POST /api/dm` returns the existing one
     * or creates it (the server never makes a second 1:1 DM for a pair).
     * Throws [ApiException] with the server code (`VALIDATION_ERROR` when
     * either side blocked the other or the account does not exist).
     */
    suspend fun createDm(userId: String): DmConversation

    /** User ids the signed-in user blocked (`GET /api/friends/blocked`). */
    suspend fun getBlocked(): Set<String>
}

class DmApiImpl @Inject constructor(
    private val httpClient: HttpClient
) : DmApi {

    override suspend fun getDms(): List<DmConversation> {
        val session = boundSession()
        val response = httpClient.get("/api/dm") { requireSession(session) }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to load direct messages")
        }

        return response.body<List<DmConversation>>().map(::withSystemFlag)
    }

    override suspend fun createDm(userId: String): DmConversation {
        val session = boundSession()
        val response = httpClient.post("/api/dm") {
            requireSession(session)
            setBody(CreateDmRequest(listOf(userId)))
        }
        response.ensureSuccess("create dm failed")
        return withSystemFlag(response.body())
    }

    override suspend fun getBlocked(): Set<String> {
        val session = boundSession()
        val response = httpClient.get("/api/friends/blocked") { requireSession(session) }
        response.ensureSuccess("blocked list failed")
        return response.body<List<BlockedUser>>().mapTo(HashSet()) { it.userId }
    }

    // C28: the Kaiku DM is recognised only by the `is_system` flags (item or participant).
    private fun withSystemFlag(dm: DmConversation): DmConversation =
        if (dm.isSystem || dm.participants.none { it.isSystem }) dm else dm.copy(isSystem = true)

    override suspend fun markAsRead(channelId: String, lastReadMessageId: String) {
        val session = boundSession()
        val response = httpClient.post("/api/dm/$channelId/read") {
            requireSession(session)
            setBody(MarkReadRequest(lastReadMessageId))
        }

        if (!response.status.isSuccess()) {
            val errorBody = runCatching { response.body<ApiErrorResponse>() }.getOrNull()
            throw ApiException(response.status, errorBody?.message ?: "Failed to mark DM as read")
        }
    }
}
