package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.session.FakeSessions
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import io.wolftown.kaiku.domain.model.LastMessagePreview
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The chat list belongs to the signed-in session: nothing of one account's
 * chats reaches the next account, also not through a sign-out that still
 * waits for the server while its tokens are stored.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class DmRepositorySessionTest {
    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 16)
    private val webSocket: KaikuWebSocket = mockk(relaxed = true) { every { events } returns this@DmRepositorySessionTest.events }
    private val api: DmApi = mockk(relaxed = true)
    private val authState: AuthState = mockk(relaxed = true) {
        every { currentUserId } returns MutableStateFlow<String?>("anna")
    }
    private val sessions = FakeSessions("anna")

    private fun TestScope.repository() =
        DmRepository(api, webSocket, KaikuJson, authState, backgroundScope, sessions.generation).also {
            events.tryEmit(ServerEvent.Ready("anna"))
        }

    private fun p(id: String) = DmParticipant(userId = id, username = id, displayName = id)
    private fun dm(id: String, vararg people: String) = DmConversation(id = id, participants = people.map(::p))
    private fun person(id: String) = EligiblePerson(id, sessions.generation.enter()!!)

    /** What the explicit sign-out does first, while the tokens are still stored: close, then clear. */
    private fun DmRepository.signOutStarts() {
        sessions.generation.close(sessions.sessionId)
        clearSession()
    }

    @Test
    fun `a conversation opened while the sign-out waits for the server never reaches the next account`() = runTest {
        coEvery { api.getDms() } returns listOf(dm("dm-carl", "anna", "carl"))
        coEvery { api.createDm("carl") } returns dm("dm-carl", "anna", "carl")
        val repo = repository()
        val carl = person("carl")
        repo.signOutStarts()

        // Anna's tokens are still stored and her screens still up: she taps once more.
        val open = async { runCatching { repo.openDirectMessage(carl) } }
        repo.loadDms()
        runCurrent()

        assertTrue("opened for a session that is being signed out", open.await().isFailure)
        coVerify(exactly = 0) { api.createDm(any()) }
        coVerify(exactly = 0) { api.getDms() }

        // The tokens go, the next account signs in: an empty list.
        sessions.switchTo("bert")
        runCurrent()
        assertEquals(emptyList<DmConversation>(), repo.conversations.value)
    }

    @Test
    fun `the advance clears the list itself, also when no teardown ran before`() = runTest {
        coEvery { api.getDms() } returns listOf(dm("dm-carl", "anna", "carl"))
        val repo = repository()
        repo.loadDms()
        assertEquals(1, repo.conversations.value.size)

        // The session expires: the hook's advance, nothing else yet.
        sessions.generation.advance()
        assertEquals(emptyList<DmConversation>(), repo.conversations.value)
    }

    @Test
    fun `a list answer of the previous account is not published for the next one`() = runTest {
        val answer = CompletableDeferred<List<DmConversation>>()
        coEvery { api.getDms() } coAnswers { answer.await() }
        val repo = repository()
        val load = launch { repo.loadDms() }
        runCurrent()

        sessions.switchTo("bert")
        answer.complete(listOf(dm("dm-carl", "anna", "carl")))
        runCurrent()

        assertEquals(emptyList<DmConversation>(), repo.conversations.value)
        load.join()
    }

    @Test
    fun `events of the previous account's socket are not added to the next account's list`() = runTest {
        val repo = repository()
        sessions.switchTo("bert")
        // Anna's socket is still on the line for a moment.
        events.emit(ServerEvent.DmCreated(buildJsonObject { put("id", "dm-x"); put("name", "x") }))
        runCurrent()
        assertEquals(emptyList<DmConversation>(), repo.conversations.value)

        // Bert's own socket: its events are his.
        events.emit(ServerEvent.Ready("bert"))
        events.emit(ServerEvent.DmCreated(buildJsonObject { put("id", "dm-y"); put("name", "y") }))
        runCurrent()
        assertEquals(listOf("dm-y"), repo.conversations.value.map { it.id })
    }

    @Test
    fun `nothing is loaded or marked read while nobody is signed in`() = runTest {
        val repo = repository()
        sessions.signOut()
        repo.loadDms()
        repo.markAsRead("dm-1")
        runCurrent()
        coVerify(exactly = 0) { api.getDms() }
        coVerify(exactly = 0) { api.markAsRead(any(), any()) }
    }

    @Test
    fun `a person confirmed for the previous account opens nothing for the next one`() = runTest {
        coEvery { api.createDm("carl") } returns dm("dm-carl", "bert", "carl")
        val repo = repository()
        val confirmedAsAnna = person("carl")
        sessions.switchTo("bert")

        val open = runCatching { repo.openDirectMessage(confirmedAsAnna) }
        assertTrue(open.isFailure)
        coVerify(exactly = 0) { api.createDm(any()) }
    }

    @Test
    fun `a read on another device that arrives while the list loads is not undone by the older answer`() = runTest {
        val unread = listOf(
            dm("dm-carl", "anna", "carl").copy(unreadCount = 3, lastMessage = LastMessagePreview("m3", "hi", createdAt = "2026-11-19T10:00:00Z")),
        )
        coEvery { api.getDms() } returns unread
        val repo = repository()
        repo.loadDms()
        assertEquals(3, repo.conversations.value.single().unreadCount)

        // A reload is out (the server took its snapshot: three unread) ...
        val answer = CompletableDeferred<List<DmConversation>>()
        coEvery { api.getDms() } coAnswers { answer.await() }
        val load = launch { repo.loadDms() }
        runCurrent()
        // ... when the other device reads them all, and its event gets here first.
        events.emit(ServerEvent.DmRead("dm-carl", lastReadMessageId = "m3"))
        runCurrent()
        assertEquals(0, repo.conversations.value.single().unreadCount)
        answer.complete(unread)
        runCurrent()
        load.join()

        assertEquals("the older answer brought the badge back", 0, repo.conversations.value.single().unreadCount)

        // A later reload (sent after the read) is taken as it is.
        coEvery { api.getDms() } returns unread.map { it.copy(unreadCount = 1, lastMessage = LastMessagePreview("m4", "new", createdAt = "2026-11-19T11:00:00Z")) }
        repo.loadDms()
        assertEquals(1, repo.conversations.value.single().unreadCount)
    }

    @Test
    fun `a read that did not reach the answer's last message leaves the server's count`() = runTest {
        val answer = CompletableDeferred<List<DmConversation>>()
        coEvery { api.getDms() } coAnswers { answer.await() }
        val repo = repository()
        val load = launch { repo.loadDms() }
        runCurrent()
        events.emit(ServerEvent.DmRead("dm-carl", lastReadMessageId = "m2"))
        answer.complete(
            listOf(dm("dm-carl", "anna", "carl").copy(unreadCount = 1, lastMessage = LastMessagePreview("m3", "hi"))),
        )
        runCurrent()
        load.join()
        assertEquals(1, repo.conversations.value.single().unreadCount)
    }
}
