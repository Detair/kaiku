package io.wolftown.kaiku.integration

import io.ktor.client.engine.mock.*
import io.ktor.client.request.*
import io.ktor.http.*
import io.mockk.*
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.api.MessageApiImpl
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AttachmentReader
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WsJson
import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.Golden
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import java.util.Collections

/**
 * Integration test for the message flow through the real HTTP stack:
 * [MessageApiImpl] over a Ktor [MockEngine] serving the server-generated
 * golden page, feeding the real [ChatRepository].
 *
 * Only the WebSocket and [GuildRepository] are mocked.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MessageFlowTest {

    private companion object {
        // Ids from server/tests/integration/android_contract_fixtures.rs
        const val CHANNEL_ID = "00000000-0000-4000-8000-00000000c001"
        const val OLDER_ID = "00000000-0000-4000-8000-00000000e001"
        const val NEWER_ID = "00000000-0000-4000-8000-00000000e002"
        const val LIVE_ID = "00000000-0000-4000-8000-00000000e003"

        const val OLDEST_ID = "00000000-0000-4000-8000-00000000e000"
    }

    private lateinit var webSocket: KaikuWebSocket
    private lateinit var guildRepository: GuildRepository
    private lateinit var tokenStorage: TokenStorage
    private lateinit var authState: AuthState

    private val eventsFlow = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)

    /** Every request the MockEngine received, in order. */
    private val requests: MutableList<HttpRequestData> =
        Collections.synchronizedList(mutableListOf())

    private val author = User(id = "user-1", username = "alice", displayName = "Alice")

    private val oldestMessage = Message(
        id = OLDEST_ID,
        channelId = CHANNEL_ID,
        author = author,
        content = "Very old message",
        createdAt = "2026-03-12T09:00:00Z"
    )

    private val sentMessage = Message(
        id = "00000000-0000-4000-8000-00000000e00f",
        channelId = CHANNEL_ID,
        author = author,
        content = "My new message",
        createdAt = "2026-03-12T10:05:00Z"
    )

    @Before
    fun setUp() {
        webSocket = mockk(relaxed = true)
        guildRepository = mockk(relaxed = true)
        tokenStorage = mockk(relaxed = true)
        authState = mockk(relaxed = true)
        every { authState.currentUserId } returns MutableStateFlow<String?>(null)
        every { webSocket.events } returns eventsFlow
        every { tokenStorage.getServerUrl() } returns "https://kaiku.example.com"
        every { tokenStorage.getAccessToken() } returns "test-access-token"
        requests.clear()
    }

    private fun jsonResponse(scope: MockRequestHandleScope, body: String) = scope.respond(
        content = body,
        status = HttpStatusCode.OK,
        headers = headersOf(HttpHeaders.ContentType, "application/json")
    )

    /**
     * Serves the golden page for the initial load, [olderPage] for
     * `before=<cursor>` requests and echoes [sentMessage] for POSTs.
     */
    private fun createRepository(
        olderPage: CursorPage<Message> = CursorPage(emptyList(), hasMore = false)
    ): ChatRepository {
        val engine = MockEngine { request ->
            requests += request
            when {
                request.method == HttpMethod.Post ->
                    jsonResponse(this, KaikuJson.encodeToString(Message.serializer(), sentMessage))

                request.url.parameters["before"] != null ->
                    jsonResponse(
                        this,
                        KaikuJson.encodeToString(
                            CursorPage.serializer(Message.serializer()),
                            olderPage
                        )
                    )

                else -> jsonResponse(this, Golden.load("messages_page.json"))
            }
        }
        val httpClient = KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient()

        // UnconfinedTestDispatcher runs the WebSocket event-collector
        // continuations synchronously, so `.value` reflects emitted
        // ServerEvents after `advanceUntilIdle()` (app/src/test/AGENTS.md).
        return ChatRepository(
            MessageApiImpl(httpClient),
            webSocket,
            KaikuJson,
            guildRepository,
            authState,
            CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()),
            mockk<AttachmentReader>(relaxed = true),
        )
    }

    private fun ChatRepository.ids(): List<String> = getMessages(CHANNEL_ID).value.map { it.id }

    // ========================================================================
    // History (REST)
    // ========================================================================

    @Test
    fun `initial load shows server page oldest first`() = runTest {
        val repository = createRepository()

        repository.subscribeToChannel(CHANNEL_ID)
        val hasMore = repository.loadMessages(CHANNEL_ID)

        verify { webSocket.send(match { it is ClientEvent.Subscribe }) }
        assertTrue(hasMore)
        // The golden page is newest first; the list must be oldest first.
        assertEquals(listOf(OLDER_ID, NEWER_ID), repository.ids())

        val request = requests.single()
        assertEquals("/api/messages/channel/$CHANNEL_ID", request.url.encodedPath)
        assertEquals("50", request.url.parameters["limit"])
        assertNull(request.url.parameters["before"])
        assertNull(request.url.parameters["after"])
    }

    @Test
    fun `load older merges before cursor and reports hasMore`() = runTest {
        val repository = createRepository(
            olderPage = CursorPage(listOf(oldestMessage), hasMore = false)
        )

        repository.loadMessages(CHANNEL_ID)
        val firstId = repository.getMessages(CHANNEL_ID).value.first().id
        val hasMore = repository.loadMessages(CHANNEL_ID, before = firstId)

        assertEquals(OLDER_ID, firstId)
        assertEquals(OLDER_ID, requests.last().url.parameters["before"])
        assertNull(requests.last().url.parameters["after"])
        assertFalse(hasMore)
        assertEquals(listOf(OLDEST_ID, OLDER_ID, NEWER_ID), repository.ids())
    }

    @Test
    fun `getMessages rejects before and after together`() = runTest {
        val engine = MockEngine { error("must not send a request") }
        val api = MessageApiImpl(KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient())

        try {
            api.getMessages(CHANNEL_ID, before = OLDER_ID, after = NEWER_ID)
            fail("Expected IllegalArgumentException")
        } catch (expected: IllegalArgumentException) {
            // ok
        }
    }

    // ========================================================================
    // Live messages (WebSocket)
    // ========================================================================

    @Test
    fun `live message is appended after loaded history`() = runTest {
        val repository = createRepository()
        repository.subscribeToChannel(CHANNEL_ID)
        repository.loadMessages(CHANNEL_ID)

        eventsFlow.emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_message_new.json")))
        advanceUntilIdle()

        assertEquals(listOf(OLDER_ID, NEWER_ID, LIVE_ID), repository.ids())
        verify(exactly = 1) { guildRepository.onMessageReceived(CHANNEL_ID, any(), any()) }
    }

    @Test
    fun `live message from the current user does not count as unread`() = runTest {
        val event = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_message_new.json"))
        val authorId = (event as ServerEvent.MessageNew).message["author"]
            ?.jsonObject?.get("id")?.jsonPrimitive?.content
        every { authState.currentUserId } returns MutableStateFlow<String?>(authorId)
        val repository = createRepository()
        repository.loadMessages(CHANNEL_ID)

        eventsFlow.emit(event)
        advanceUntilIdle()

        verify(exactly = 1) {
            guildRepository.onMessageReceived(CHANNEL_ID, LIVE_ID, countUnread = false)
        }
    }

    @Test
    fun `duplicate WebSocket message is not added twice`() = runTest {
        val repository = createRepository()
        repository.loadMessages(CHANNEL_ID)

        val event = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_message_new.json"))
        eventsFlow.emit(event)
        eventsFlow.emit(event)
        advanceUntilIdle()

        assertEquals(listOf(OLDER_ID, NEWER_ID, LIVE_ID), repository.ids())
        verify(exactly = 1) { guildRepository.onMessageReceived(CHANNEL_ID, any(), any()) }
    }

    @Test
    fun `sending message adds it after loaded history`() = runTest {
        val repository = createRepository()
        repository.loadMessages(CHANNEL_ID)

        repository.sendMessage(CHANNEL_ID, "My new message")

        val post = requests.last()
        assertEquals(HttpMethod.Post, post.method)
        assertEquals("/api/messages/channel/$CHANNEL_ID", post.url.encodedPath)
        assertEquals(listOf(OLDER_ID, NEWER_ID, sentMessage.id), repository.ids())
    }

    @Test
    fun `edit event updates message content in list`() = runTest {
        val repository = createRepository()
        repository.loadMessages(CHANNEL_ID)

        eventsFlow.emit(
            ServerEvent.MessageEdit(
                channelId = CHANNEL_ID,
                messageId = OLDER_ID,
                content = "Updated content",
                editedAt = "2026-03-12T10:10:00Z"
            )
        )
        advanceUntilIdle()

        val edited = repository.getMessages(CHANNEL_ID).value.single { it.id == OLDER_ID }
        assertEquals("Updated content", edited.content)
        assertEquals("2026-03-12T10:10:00Z", edited.editedAt)
    }

    @Test
    fun `delete event removes message from list`() = runTest {
        val repository = createRepository()
        repository.loadMessages(CHANNEL_ID)

        eventsFlow.emit(ServerEvent.MessageDelete(channelId = CHANNEL_ID, messageId = OLDER_ID))
        advanceUntilIdle()

        assertEquals(listOf(NEWER_ID), repository.ids())
    }

    // ========================================================================
    // Typing indicators and subscriptions
    // ========================================================================

    @Test
    fun `typing start and stop update typing set`() = runTest {
        val repository = createRepository()
        repository.subscribeToChannel(CHANNEL_ID)

        eventsFlow.emit(ServerEvent.TypingStart(channelId = CHANNEL_ID, userId = "user-2"))
        // No advanceUntilIdle here: it would run the 8 s typing expiry.
        assertTrue(repository.getTypingUsers(CHANNEL_ID).value.containsKey("user-2"))

        eventsFlow.emit(ServerEvent.TypingStop(channelId = CHANNEL_ID, userId = "user-2"))
        advanceUntilIdle()
        assertFalse(repository.getTypingUsers(CHANNEL_ID).value.containsKey("user-2"))
    }

    @Test
    fun `send typing indicator debounces within 3 seconds`() = runTest {
        val repository = createRepository()
        repository.subscribeToChannel(CHANNEL_ID)

        repository.sendTypingIndicator(CHANNEL_ID)
        repository.sendTypingIndicator(CHANNEL_ID) // debounced
        repository.sendTypingIndicator(CHANNEL_ID) // debounced

        verify(exactly = 1) { webSocket.send(match { it is ClientEvent.Typing }) }
    }

    @Test
    fun `unsubscribe sends WebSocket event`() = runTest {
        val repository = createRepository()
        repository.subscribeToChannel(CHANNEL_ID)
        repository.unsubscribeFromChannel(CHANNEL_ID)

        verify { webSocket.send(match { it is ClientEvent.Unsubscribe }) }
    }
}
