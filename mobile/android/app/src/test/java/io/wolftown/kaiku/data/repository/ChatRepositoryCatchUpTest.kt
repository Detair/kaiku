package io.wolftown.kaiku.data.repository

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.MockRequestHandleScope
import io.ktor.client.engine.mock.respond
import io.ktor.client.request.HttpRequestData
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.clearMocks
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.api.MessageApiImpl
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
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
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.Collections

/**
 * Reconnect catch-up and re-subscribe in [ChatRepository], through the real
 * HTTP stack: [MessageApiImpl] over a Ktor [MockEngine]. The initial load is
 * the server-generated golden page (`messages_page.json`); forward pages are
 * built from [Message]s. Only the WebSocket and [GuildRepository] are mocked.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class ChatRepositoryCatchUpTest {

    private companion object {
        // Ids from server/tests/integration/android_contract_fixtures.rs
        const val CHANNEL_ID = "00000000-0000-4000-8000-00000000c001"
        const val OLDER_ID = "00000000-0000-4000-8000-00000000e001"
        const val NEWER_ID = "00000000-0000-4000-8000-00000000e002"
        const val LIVE_ID = "00000000-0000-4000-8000-00000000e003"
    }

    private lateinit var webSocket: KaikuWebSocket
    private lateinit var guildRepository: GuildRepository
    private lateinit var tokenStorage: TokenStorage
    private lateinit var authState: AuthState

    private val eventsFlow = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)

    private val requests: MutableList<HttpRequestData> =
        Collections.synchronizedList(mutableListOf())

    private val author = User(id = "user-1", username = "alice", displayName = "Alice")

    /** A message after the golden page (which ends at 10:01:00). */
    private fun missed(n: Int) = Message(
        id = "00000000-0000-4000-8000-0000000f%04d".format(n),
        channelId = CHANNEL_ID,
        author = author,
        content = "Missed $n",
        createdAt = "2026-03-12T11:%02d:%02dZ".format(n / 60, n % 60),
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

    private fun pageJson(page: CursorPage<Message>) =
        KaikuJson.encodeToString(CursorPage.serializer(Message.serializer()), page)

    /**
     * Requests without `after` get the golden page. `after` requests are
     * answered by [afterPage] with the cursor and the 0-based index of the
     * `after` request; returning null answers 400 "Unknown cursor".
     */
    private fun createRepository(
        onLatestPageRequest: () -> Unit = {},
        afterPage: (after: String, index: Int) -> CursorPage<Message>? = { _, _ ->
            CursorPage(emptyList(), hasMore = false)
        },
    ): ChatRepository {
        var afterIndex = 0
        val engine = MockEngine { request ->
            requests += request
            val after = request.url.parameters["after"]
            if (after == null) {
                onLatestPageRequest()
                jsonResponse(this, Golden.load("messages_page.json"))
            } else {
                val page = afterPage(after, afterIndex++)
                if (page == null) {
                    respond(
                        content = """{"error":"VALIDATION","message":"Unknown cursor"}""",
                        status = HttpStatusCode.BadRequest,
                        headers = headersOf(HttpHeaders.ContentType, "application/json")
                    )
                } else {
                    jsonResponse(this, pageJson(page))
                }
            }
        }
        val httpClient = KaikuHttpClient.forTesting(tokenStorage, authState, engine).activeClient()
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

    private fun ChatRepository.ids(channelId: String = CHANNEL_ID): List<String> =
        getMessages(channelId).value.map { it.id }

    private fun afterRequests() = requests.filter { it.url.parameters["after"] != null }

    // -- Re-subscribe ---------------------------------------------------------

    @Test
    fun `ready resubscribes all open channels`() = runTest {
        val repository = createRepository()
        repository.subscribeToChannel("a")
        repository.subscribeToChannel("b")
        repository.unsubscribeFromChannel("b")
        clearMocks(webSocket, answers = false, recordedCalls = true)

        val resent = repository.resubscribeAll()

        assertEquals(setOf("a"), resent)
        verify(exactly = 1) { webSocket.send(ClientEvent.Subscribe("a")) }
        verify(exactly = 0) { webSocket.send(ClientEvent.Subscribe("b")) }
    }

    @Test
    fun `closing an open dm screen keeps its background subscription`() = runTest {
        val repository = createRepository()
        repository.setDmChannels(listOf("dm-1"))
        repository.subscribeToChannel("dm-1")
        clearMocks(webSocket, answers = false, recordedCalls = true)

        repository.unsubscribeFromChannel("dm-1")

        verify(exactly = 0) { webSocket.send(ClientEvent.Unsubscribe("dm-1")) }
        assertEquals(setOf("dm-1"), repository.resubscribeAll())
    }

    @Test
    fun `set dm channels subscribes new ids only and drops removed ones`() = runTest {
        val repository = createRepository()
        repository.setDmChannels(listOf("dm-1", "dm-2"))
        clearMocks(webSocket, answers = false, recordedCalls = true)

        val sent = repository.setDmChannels(listOf("dm-2", "dm-3"))

        assertEquals(setOf("dm-3"), sent)
        verify(exactly = 1) { webSocket.send(ClientEvent.Subscribe("dm-3")) }
        verify(exactly = 0) { webSocket.send(ClientEvent.Subscribe("dm-2")) }
        verify(exactly = 1) { webSocket.send(ClientEvent.Unsubscribe("dm-1")) }
    }

    // -- Catch-up -------------------------------------------------------------

    @Test
    fun `catch up requests after last known id and merges ascending`() = runTest {
        val repository = createRepository { _, _ ->
            // Deliberately newest first: the merge must sort.
            CursorPage(listOf(missed(2), missed(1)), hasMore = false)
        }
        repository.loadMessages(CHANNEL_ID)

        repository.catchUp(CHANNEL_ID)

        val request = afterRequests().single()
        assertEquals(NEWER_ID, request.url.parameters["after"])
        assertNull(request.url.parameters["before"])
        assertEquals("100", request.url.parameters["limit"])
        assertEquals(listOf(OLDER_ID, NEWER_ID, missed(1).id, missed(2).id), repository.ids())
        verify(exactly = 2) { guildRepository.onMessageReceived(CHANNEL_ID, any(), any()) }
    }

    @Test
    fun `catch up does not count the users own messages as unread`() = runTest {
        every { authState.currentUserId } returns MutableStateFlow<String?>(author.id)
        val repository = createRepository { _, _ ->
            CursorPage(listOf(missed(1)), hasMore = false)
        }
        repository.loadMessages(CHANNEL_ID)

        repository.catchUp(CHANNEL_ID)

        verify(exactly = 1) {
            guildRepository.onMessageReceived(CHANNEL_ID, missed(1).id, countUnread = false)
        }
    }

    @Test
    fun `catch up pages until has_more false`() = runTest {
        val repository = createRepository { _, index ->
            when (index) {
                0 -> CursorPage(listOf(missed(1), missed(2)), hasMore = true, nextCursor = missed(2).id)
                else -> CursorPage(listOf(missed(3)), hasMore = false)
            }
        }
        repository.loadMessages(CHANNEL_ID)

        repository.catchUp(CHANNEL_ID)

        assertEquals(listOf(NEWER_ID, missed(2).id), afterRequests().map { it.url.parameters["after"] })
        assertEquals(
            listOf(OLDER_ID, NEWER_ID, missed(1).id, missed(2).id, missed(3).id),
            repository.ids()
        )
        verify(exactly = 3) { guildRepository.onMessageReceived(CHANNEL_ID, any(), any()) }
    }

    @Test
    fun `catch up follows next_cursor past an empty filtered page`() = runTest {
        // The server computes next_cursor before dropping blocked users'
        // messages, so a page can be empty while has_more is true.
        val repository = createRepository { _, index ->
            when (index) {
                0 -> CursorPage(emptyList(), hasMore = true, nextCursor = "x")
                else -> CursorPage(listOf(missed(1)), hasMore = false)
            }
        }
        repository.loadMessages(CHANNEL_ID)

        repository.catchUp(CHANNEL_ID)

        assertEquals(listOf(NEWER_ID, "x"), afterRequests().map { it.url.parameters["after"] })
        assertEquals(listOf(OLDER_ID, NEWER_ID, missed(1).id), repository.ids())
        verify(exactly = 1) { guildRepository.onMessageReceived(CHANNEL_ID, any(), any()) }
    }

    @Test
    fun `catch up stops when has_more comes without next_cursor`() = runTest {
        val repository = createRepository { _, _ -> CursorPage(listOf(missed(1)), hasMore = true) }
        repository.loadMessages(CHANNEL_ID)

        repository.catchUp(CHANNEL_ID)

        assertEquals(1, afterRequests().size)
        assertEquals(listOf(OLDER_ID, NEWER_ID, missed(1).id), repository.ids())
    }

    @Test
    fun `catch up beyond page cap reloads latest page`() = runTest {
        val repository = createRepository { _, index ->
            CursorPage(listOf(missed(index)), hasMore = true, nextCursor = missed(index).id)
        }
        repository.loadMessages(CHANNEL_ID)
        requests.clear()

        repository.catchUp(CHANNEL_ID)

        assertEquals(ChatRepository.CATCH_UP_MAX_PAGES, afterRequests().size)
        val last = requests.last()
        assertNull(last.url.parameters["after"])
        assertNull(last.url.parameters["before"])
        assertEquals(ChatRepository.CATCH_UP_MAX_PAGES + 1, requests.size)
        assertTrue(repository.ids().containsAll(listOf(OLDER_ID, NEWER_ID)))
    }

    @Test
    fun `catch up skips channels never loaded`() = runTest {
        val repository = createRepository()
        repository.subscribeToChannel("never-loaded") // flow exists, list empty

        repository.catchUp("never-loaded")
        repository.catchUp("unknown")

        assertTrue(requests.isEmpty())
    }

    @Test
    fun `catch up uses the newest id from before the reconnect`() = runTest {
        val repository = createRepository { _, _ -> CursorPage(listOf(missed(1)), hasMore = false) }
        repository.loadMessages(CHANNEL_ID)

        // New socket: Ready, then a live message lands before the catch-up runs.
        eventsFlow.emit(ServerEvent.Ready(userId = "user-1"))
        eventsFlow.emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_message_new.json")))
        repository.catchUp(CHANNEL_ID)

        assertEquals(NEWER_ID, afterRequests().single().url.parameters["after"])
        assertEquals(listOf(OLDER_ID, NEWER_ID, LIVE_ID, missed(1).id), repository.ids())
    }

    @Test
    fun `catch up with rejected cursor reloads latest page`() = runTest {
        val repository = createRepository { _, _ -> null }
        repository.loadMessages(CHANNEL_ID)
        requests.clear()

        repository.catchUp(CHANNEL_ID)

        assertEquals(2, requests.size)
        assertNull(requests.last().url.parameters["after"])
        assertEquals(listOf(OLDER_ID, NEWER_ID), repository.ids())
    }

    // -- Ready watermark ------------------------------------------------------

    @Test
    fun `ready fixture with server_time decodes and is stored`() = runTest {
        val repository = createRepository()
        assertNull(repository.lastServerTime.value)

        eventsFlow.emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_ready.json")))

        assertEquals("2026-03-12T10:03:00.250Z", repository.lastServerTime.value)
    }

    // -- Initial load ---------------------------------------------------------

    @Test
    fun `initial load keeps a live message that arrived during the request`() = runTest {
        val live = WsJson.decodeFromString<ServerEvent>(Golden.load("ws_message_new.json"))
        val repository = createRepository(onLatestPageRequest = { eventsFlow.tryEmit(live) })

        repository.loadMessages(CHANNEL_ID)

        assertEquals(listOf(OLDER_ID, NEWER_ID, LIVE_ID), repository.ids())
    }

    @Test
    fun `initial load still drops messages older than the page`() = runTest {
        val stale = missed(0).copy(id = "stale", createdAt = "2026-03-12T09:00:00Z")
        val repository = createRepository()
        val payload = KaikuJson.encodeToJsonElement(Message.serializer(), stale).jsonObject
        eventsFlow.emit(ServerEvent.MessageNew(CHANNEL_ID, payload))
        assertEquals(listOf("stale"), repository.ids())

        repository.loadMessages(CHANNEL_ID)

        assertEquals(listOf(OLDER_ID, NEWER_ID), repository.ids())
    }

    // -- Fix round 2 ----------------------------------------------------------

    @Test
    fun `set dm channels does not unsubscribe a dm tracked after the mark`() = runTest {
        val repository = createRepository()
        repository.setDmChannels(listOf("dm-1"))
        val mark = repository.dmTrackingMark()
        repository.trackDmChannel("dm-new") // dm_created while GET /api/dm was in flight
        clearMocks(webSocket, answers = false, recordedCalls = true)

        repository.setDmChannels(listOf("dm-1"), keepTrackedAfter = mark)

        verify(exactly = 0) { webSocket.send(ClientEvent.Unsubscribe("dm-new")) }
        assertEquals(setOf("dm-1", "dm-new"), repository.resubscribeAll())
    }

    @Test
    fun `unconsumed anchor survives a second ready`() = runTest {
        val repository = createRepository { _, _ -> CursorPage(listOf(missed(1)), hasMore = false) }
        repository.loadMessages(CHANNEL_ID)

        // Socket 1: Ready, a live message, then the socket drops before catch-up.
        eventsFlow.emit(ServerEvent.Ready(userId = "user-1"))
        eventsFlow.emit(WsJson.decodeFromString<ServerEvent>(Golden.load("ws_message_new.json")))
        // Socket 2.
        eventsFlow.emit(ServerEvent.Ready(userId = "user-1"))
        repository.catchUp(CHANNEL_ID)

        assertEquals(NEWER_ID, afterRequests().single().url.parameters["after"])
    }

    @Test
    fun `completed catch up releases its anchor for the next reconnect`() = runTest {
        val repository = createRepository { _, index ->
            CursorPage(if (index == 0) listOf(missed(1)) else emptyList(), hasMore = false)
        }
        repository.loadMessages(CHANNEL_ID)
        eventsFlow.emit(ServerEvent.Ready(userId = "user-1"))
        repository.catchUp(CHANNEL_ID)

        eventsFlow.emit(ServerEvent.Ready(userId = "user-1"))
        repository.catchUp(CHANNEL_ID)

        assertEquals(listOf(NEWER_ID, missed(1).id), afterRequests().map { it.url.parameters["after"] })
    }

    @Test
    fun `initial load keeps a live message with the same timestamp and a smaller id`() = runTest {
        // Same createdAt as the golden page's newest item (e002), smaller id.
        val sameTime = missed(0).copy(
            id = "00000000-0000-4000-8000-00000000e000",
            createdAt = "2026-03-12T10:01:00Z",
        )
        val payload = KaikuJson.encodeToJsonElement(Message.serializer(), sameTime).jsonObject
        val repository = createRepository(
            onLatestPageRequest = { eventsFlow.tryEmit(ServerEvent.MessageNew(CHANNEL_ID, payload)) }
        )

        repository.loadMessages(CHANNEL_ID)

        assertEquals(listOf(OLDER_ID, sameTime.id, NEWER_ID), repository.ids())
    }
}
