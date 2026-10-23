package io.wolftown.kaiku.data.api

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.client.engine.mock.toByteArray
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.LastMessagePreview
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class DmApiTest {

    private val tokenStorage: TokenStorage = mockk(relaxed = true)
    private val authState: AuthState = mockk(relaxed = true)

    init {
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com"
    }

    @Test
    fun `mark dm read sends last_read_message_id body`() = runTest {
        var method: HttpMethod? = null
        var path: String? = null
        var body: String? = null
        val engine = MockEngine { request ->
            method = request.method
            path = request.url.encodedPath
            body = String(request.body.toByteArray())
            respond(content = "", status = HttpStatusCode.NoContent)
        }
        val client = KaikuHttpClient.forTesting(tokenStorage, authState, engine)

        DmApiImpl(client.activeClient()).markAsRead("dm-1", "m1")

        assertEquals(HttpMethod.Post, method)
        assertEquals("/api/dm/dm-1/read", path)
        assertTrue(body.orEmpty(), body.orEmpty().contains("\"last_read_message_id\":\"m1\""))
    }

    @Test
    fun `mark dm read without last message skips request`() = runTest {
        val api: DmApi = mockk(relaxed = true)
        val dm = DmConversation(id = "dm-1", unreadCount = 2, lastMessage = null)
        coEvery { api.getDms() } returns listOf(dm)
        val repository = DmRepository(api)
        repository.loadDms()

        repository.markAsRead("dm-1")

        coVerify(exactly = 0) { api.markAsRead(any(), any()) }
        assertEquals(0, repository.conversations.value.single().unreadCount)
    }

    @Test
    fun `mark dm read passes last message id from conversation`() = runTest {
        val api: DmApi = mockk(relaxed = true)
        val dm = DmConversation(
            id = "dm-1",
            unreadCount = 1,
            lastMessage = LastMessagePreview(id = "m1", content = "hi"),
        )
        coEvery { api.getDms() } returns listOf(dm)
        val repository = DmRepository(api)
        repository.loadDms()

        repository.markAsRead("dm-1")

        coVerify(exactly = 1) { api.markAsRead("dm-1", "m1") }
    }
}
