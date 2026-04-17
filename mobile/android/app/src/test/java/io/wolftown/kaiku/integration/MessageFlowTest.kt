package io.wolftown.kaiku.integration

import io.mockk.*
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.ws.ClientEvent
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.encodeToJsonElement
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

/**
 * Integration test verifying the message flow:
 * subscribe → receive messages → display → send → optimistic update.
 *
 * Uses mocked API and WebSocket but exercises real ChatRepository logic.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MessageFlowTest {

    private lateinit var messageApi: MessageApi
    private lateinit var webSocket: KaikuWebSocket
    private lateinit var chatRepository: ChatRepository

    private val eventsFlow = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)
    private val json = Json { ignoreUnknownKeys = true }

    private val testAuthor = User(
        id = "user-1",
        username = "alice",
        displayName = "Alice"
    )

    private val initialMessages = listOf(
        Message(
            id = "msg-1",
            channelId = "ch-1",
            author = testAuthor,
            content = "First message",
            createdAt = "2026-03-12T10:00:00Z"
        ),
        Message(
            id = "msg-2",
            channelId = "ch-1",
            author = testAuthor,
            content = "Second message",
            createdAt = "2026-03-12T10:01:00Z"
        )
    )

    @Before
    fun setUp() {
        messageApi = mockk(relaxed = true)
        webSocket = mockk(relaxed = true)
        every { webSocket.events } returns eventsFlow

        // UnconfinedTestDispatcher runs the WebSocket event-collector
        // continuations synchronously on the calling thread, so `.value`
        // reflects emitted ServerEvents after `advanceUntilIdle()`. See
        // app/src/test/AGENTS.md ("inject the scope").
        chatRepository = ChatRepository(
            messageApi,
            webSocket,
            json,
            CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher()),
        )
    }

    // ========================================================================
    // Subscribe and load
    // ========================================================================

    @Test
    fun `subscribe sends WebSocket event and loads messages`() = runTest {
        coEvery { messageApi.getMessages("ch-1", null) } returns initialMessages

        chatRepository.subscribeToChannel("ch-1")
        chatRepository.loadMessages("ch-1")

        verify { webSocket.send(match { it is ClientEvent.Subscribe }) }

        val messages = chatRepository.getMessages("ch-1").value
        assertEquals(2, messages.size)
        assertEquals("msg-1", messages[0].id)
        assertEquals("msg-2", messages[1].id)
    }

    // ========================================================================
    // Receive message via WebSocket
    // ========================================================================

    @Test
    fun `new WebSocket message appears in message list`() = runTest {
        coEvery { messageApi.getMessages("ch-1", null) } returns initialMessages

        chatRepository.subscribeToChannel("ch-1")
        chatRepository.loadMessages("ch-1")

        assertEquals(2, chatRepository.getMessages("ch-1").value.size)

        // Simulate a new message from WebSocket
        val newMessage = Message(
            id = "msg-3",
            channelId = "ch-1",
            author = User(id = "user-2", username = "bob", displayName = "Bob"),
            content = "Hello from WebSocket!",
            createdAt = "2026-03-12T10:02:00Z"
        )
        val messageJson = json.encodeToJsonElement(newMessage).jsonObject
        eventsFlow.emit(ServerEvent.MessageNew(channelId = "ch-1", message = messageJson))
        advanceUntilIdle()

        val messages = chatRepository.getMessages("ch-1").value
        assertEquals(3, messages.size)
        assertEquals("msg-3", messages[2].id)
        assertEquals("Hello from WebSocket!", messages[2].content)
    }

    @Test
    fun `duplicate WebSocket message is not added twice`() = runTest {
        coEvery { messageApi.getMessages("ch-1", null) } returns initialMessages

        chatRepository.subscribeToChannel("ch-1")
        chatRepository.loadMessages("ch-1")

        // Re-emit an existing message
        val existingMessage = initialMessages[0]
        val messageJson = json.encodeToJsonElement(existingMessage).jsonObject
        eventsFlow.emit(ServerEvent.MessageNew(channelId = "ch-1", message = messageJson))
        advanceUntilIdle()

        // Should still be 2, not 3
        assertEquals(2, chatRepository.getMessages("ch-1").value.size)
    }

    // ========================================================================
    // Send message → optimistic update
    // ========================================================================

    @Test
    fun `sending message calls API and adds to list optimistically`() = runTest {
        val sentMessage = Message(
            id = "msg-new",
            channelId = "ch-1",
            author = testAuthor,
            content = "My new message",
            createdAt = "2026-03-12T10:05:00Z"
        )
        coEvery { messageApi.getMessages("ch-1", null) } returns initialMessages
        coEvery { messageApi.sendMessage("ch-1", "My new message") } returns sentMessage

        chatRepository.subscribeToChannel("ch-1")
        chatRepository.loadMessages("ch-1")

        chatRepository.sendMessage("ch-1", "My new message")

        coVerify { messageApi.sendMessage("ch-1", "My new message") }

        val messages = chatRepository.getMessages("ch-1").value
        assertEquals(3, messages.size)
        assertEquals("msg-new", messages[2].id)
    }

    // ========================================================================
    // Edit message via WebSocket
    // ========================================================================

    @Test
    fun `edit event updates message content in list`() = runTest {
        coEvery { messageApi.getMessages("ch-1", null) } returns initialMessages

        chatRepository.subscribeToChannel("ch-1")
        chatRepository.loadMessages("ch-1")

        eventsFlow.emit(
            ServerEvent.MessageEdit(
                channelId = "ch-1",
                messageId = "msg-1",
                content = "Updated content",
                editedAt = "2026-03-12T10:10:00Z"
            )
        )
        advanceUntilIdle()

        val messages = chatRepository.getMessages("ch-1").value
        assertEquals("Updated content", messages[0].content)
        assertEquals("2026-03-12T10:10:00Z", messages[0].editedAt)
    }

    // ========================================================================
    // Delete message via WebSocket
    // ========================================================================

    @Test
    fun `delete event removes message from list`() = runTest {
        coEvery { messageApi.getMessages("ch-1", null) } returns initialMessages

        chatRepository.subscribeToChannel("ch-1")
        chatRepository.loadMessages("ch-1")

        assertEquals(2, chatRepository.getMessages("ch-1").value.size)

        eventsFlow.emit(
            ServerEvent.MessageDelete(channelId = "ch-1", messageId = "msg-1")
        )
        advanceUntilIdle()

        val messages = chatRepository.getMessages("ch-1").value
        assertEquals(1, messages.size)
        assertEquals("msg-2", messages[0].id)
    }

    // ========================================================================
    // Pagination (load more)
    // ========================================================================

    @Test
    fun `loading older messages prepends to list`() = runTest {
        val olderMessages = listOf(
            Message(
                id = "msg-0",
                channelId = "ch-1",
                author = testAuthor,
                content = "Very old message",
                createdAt = "2026-03-12T09:00:00Z"
            )
        )
        coEvery { messageApi.getMessages("ch-1", null) } returns initialMessages
        coEvery { messageApi.getMessages("ch-1", "msg-1") } returns olderMessages

        chatRepository.subscribeToChannel("ch-1")
        chatRepository.loadMessages("ch-1")
        chatRepository.loadMessages("ch-1", before = "msg-1")

        val messages = chatRepository.getMessages("ch-1").value
        assertEquals(3, messages.size)
        assertEquals("msg-0", messages[0].id)
        assertEquals("msg-1", messages[1].id)
    }

    // ========================================================================
    // Typing indicators
    // ========================================================================

    @Test
    fun `typing start adds user to typing set`() = runTest {
        chatRepository.subscribeToChannel("ch-1")

        eventsFlow.emit(ServerEvent.TypingStart(channelId = "ch-1", userId = "user-2"))
        advanceUntilIdle()

        val typing = chatRepository.getTypingUsers("ch-1").value
        assertTrue(typing.contains("user-2"))
    }

    @Test
    fun `typing stop removes user from typing set`() = runTest {
        chatRepository.subscribeToChannel("ch-1")

        eventsFlow.emit(ServerEvent.TypingStart(channelId = "ch-1", userId = "user-2"))
        advanceUntilIdle()
        assertTrue(chatRepository.getTypingUsers("ch-1").value.contains("user-2"))

        eventsFlow.emit(ServerEvent.TypingStop(channelId = "ch-1", userId = "user-2"))
        advanceUntilIdle()

        assertFalse(chatRepository.getTypingUsers("ch-1").value.contains("user-2"))
    }

    @Test
    fun `send typing indicator debounces within 3 seconds`() = runTest {
        chatRepository.subscribeToChannel("ch-1")

        chatRepository.sendTypingIndicator("ch-1")
        chatRepository.sendTypingIndicator("ch-1") // Should be debounced
        chatRepository.sendTypingIndicator("ch-1") // Should be debounced

        // Only one Typing event should be sent
        verify(exactly = 1) { webSocket.send(match { it is ClientEvent.Typing }) }
    }

    // ========================================================================
    // Unsubscribe
    // ========================================================================

    @Test
    fun `unsubscribe sends WebSocket event`() = runTest {
        chatRepository.subscribeToChannel("ch-1")
        chatRepository.unsubscribeFromChannel("ch-1")

        verify { webSocket.send(match { it is ClientEvent.Unsubscribe }) }
    }
}
