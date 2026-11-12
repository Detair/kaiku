package io.wolftown.kaiku.ui.channel

import android.net.Uri
import androidx.lifecycle.SavedStateHandle
import app.cash.turbine.test
import io.mockk.*
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.data.ws.RealtimeConnectionManager
import io.wolftown.kaiku.data.ws.RealtimeStatus
import io.wolftown.kaiku.domain.ChannelTitle
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.push.ActiveChannelTracker
import io.wolftown.kaiku.push.NotificationLedger
import io.wolftown.kaiku.push.NotificationSink
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.*
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.initializer
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

@OptIn(ExperimentalCoroutinesApi::class)
class TextChannelViewModelTest {

    private lateinit var chatRepository: ChatRepository
    private lateinit var guildRepository: GuildRepository
    private lateinit var dmRepository: DmRepository
    private lateinit var authState: AuthState
    private lateinit var realtimeManager: RealtimeConnectionManager
    private val statusFlow = MutableStateFlow(RealtimeStatus.Connected)
    private lateinit var viewModel: TextChannelViewModel
    private lateinit var savedStateHandle: SavedStateHandle
    private val activeChannelTracker = ActiveChannelTracker()
    private val notificationSink: NotificationSink = mockk(relaxed = true)
    private val notificationLedger: NotificationLedger = mockk(relaxed = true)

    private val testDispatcher = StandardTestDispatcher()

    private val messagesFlow = MutableStateFlow<List<Message>>(emptyList())
    private val channelsFlow = MutableStateFlow<List<Channel>>(emptyList())
    private val conversationsFlow = MutableStateFlow<List<DmConversation>>(emptyList())
    private val currentUserIdFlow = MutableStateFlow<String?>("me")
    private val typingUsersFlow = MutableStateFlow<Map<String, Long>>(emptyMap())

    private val testAuthor = User(
        id = "user-1",
        username = "testuser",
        displayName = "Test User"
    )

    private val sampleMessages = listOf(
        Message(
            id = "msg-1",
            channelId = "ch-1",
            author = testAuthor,
            content = "Hello world",
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
        Dispatchers.setMain(testDispatcher)
        chatRepository = mockk(relaxed = true)
        guildRepository = mockk(relaxed = true)
        dmRepository = mockk(relaxed = true)
        authState = mockk(relaxed = true)
        realtimeManager = mockk(relaxed = true)
        every { realtimeManager.status } returns statusFlow
        every { guildRepository.channels } returns channelsFlow
        every { dmRepository.conversations } returns conversationsFlow
        every { authState.currentUserId } returns currentUserIdFlow
        every { authState.session } returns MutableStateFlow(io.wolftown.kaiku.data.local.AuthSession.LoggedIn("me", 1L))
        every { authState.endedSessions } returns kotlinx.coroutines.flow.MutableSharedFlow()
        savedStateHandle = SavedStateHandle(mapOf("channelId" to "ch-1"))

        every { chatRepository.getMessages("ch-1") } returns messagesFlow
        every { chatRepository.getTypingUsers("ch-1") } returns typingUsersFlow
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    /** App-scoped sender; outlives every view model of a test (AND-B-3). */
    private lateinit var outgoing: io.wolftown.kaiku.data.repository.OutgoingMessages
    private var nextLocalId = 0

    private fun createViewModel(): TextChannelViewModel {
        if (!::outgoing.isInitialized) {
            outgoing = io.wolftown.kaiku.data.repository.OutgoingMessages(
                chatRepository, authState,
                kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + testDispatcher),
            ) { "local-${nextLocalId++}" }
        }
        // Through a real ViewModelStore, so leaving the screen can be simulated
        // with store.clear() (cancels viewModelScope, like navigating back).
        val store = androidx.lifecycle.ViewModelStore()
        val handle = savedStateHandle
        val vm = androidx.lifecycle.ViewModelProvider.create(
            store,
            androidx.lifecycle.viewmodel.viewModelFactory {
                initializer {
                    TextChannelViewModel(
                        chatRepository, guildRepository, dmRepository, authState, realtimeManager,
                        activeChannelTracker, notificationSink, notificationLedger, outgoing, handle
                    )
                }
            },
        )[TextChannelViewModel::class]
        stores[vm] = store
        return vm
    }

    private val stores = HashMap<TextChannelViewModel, androidx.lifecycle.ViewModelStore>()

    /** The user leaves the chat: the store is cleared and viewModelScope is cancelled. */
    private fun leave(vm: TextChannelViewModel) {
        stores.remove(vm)!!.clear()
        assertFalse(vm.viewModelScope.isActive)
    }

    // ========================================================================
    // 1. Loads message history on init
    // ========================================================================

    @Test
    fun `loads message history on init`() = runTest {
        coEvery { chatRepository.loadMessages("ch-1", null) } coAnswers {
            messagesFlow.value = sampleMessages
            false
        }

        viewModel = createViewModel()
        advanceUntilIdle()

        coVerify { chatRepository.loadMessages("ch-1", null) }
        assertEquals(sampleMessages, viewModel.messages.value)
    }

    // ========================================================================
    // 2. Subscribes to channel on init
    // ========================================================================

    @Test
    fun `subscribes to channel on init`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false

        viewModel = createViewModel()
        advanceUntilIdle()

        verify { chatRepository.subscribeToChannel("ch-1") }
    }

    // ========================================================================
    // 3. New WebSocket message appends to list
    // ========================================================================

    @Test
    fun `new WebSocket message appends to list`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } coAnswers {
            messagesFlow.value = sampleMessages
            false
        }

        viewModel = createViewModel()
        advanceUntilIdle()

        assertEquals(2, viewModel.messages.value.size)

        // Simulate a new message arriving via WebSocket (ChatRepository updates the flow)
        val newMessage = Message(
            id = "msg-3",
            channelId = "ch-1",
            author = testAuthor,
            content = "New message from WS",
            createdAt = "2026-03-12T10:02:00Z"
        )
        messagesFlow.value = sampleMessages + newMessage
        advanceUntilIdle()

        assertEquals(3, viewModel.messages.value.size)
        assertEquals("msg-3", viewModel.messages.value.last().id)
    }

    // ========================================================================
    // 4. Sending message calls repository and clears input
    // ========================================================================

    @Test
    fun `sending message calls repository and clears input`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false
        coEvery { chatRepository.sendMessage("ch-1", "Hello!", 1L) } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onMessageInputChanged("Hello!")
        assertEquals("Hello!", viewModel.messageInput.value)

        viewModel.onSendMessage()
        advanceUntilIdle()

        coVerify { chatRepository.sendMessage("ch-1", "Hello!", 1L) }
        assertEquals("", viewModel.messageInput.value)
    }

    // ========================================================================
    // 5. Edit and delete update the message list
    // ========================================================================

    @Test
    fun `edit message calls repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } coAnswers {
            messagesFlow.value = sampleMessages
            false
        }
        coEvery { chatRepository.editMessage("msg-1", "Updated content") } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onEditMessage("msg-1", "Updated content")
        advanceUntilIdle()

        coVerify { chatRepository.editMessage("msg-1", "Updated content") }
    }

    @Test
    fun `delete message calls repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } coAnswers {
            messagesFlow.value = sampleMessages
            false
        }
        coEvery { chatRepository.deleteMessage("ch-1", "msg-1") } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onDeleteMessage("msg-1")
        advanceUntilIdle()

        coVerify { chatRepository.deleteMessage("ch-1", "msg-1") }
    }

    // ========================================================================
    // 6. Loading state during initial load
    // ========================================================================

    @Test
    fun `loading state during initial load`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } coAnswers {
            messagesFlow.value = sampleMessages
            false
        }

        viewModel = createViewModel()

        viewModel.isLoading.test {
            val first = awaitItem()
            if (first) {
                // Loading is true, wait for it to become false
                val second = awaitItem()
                assertFalse("Expected loading to be false after messages loaded", second)
            } else {
                // Already resolved quickly
                testScheduler.advanceUntilIdle()
            }
        }
    }

    // ========================================================================
    // 7. Unsubscribes on ViewModel cleared
    // ========================================================================

    @Test
    fun `unsubscribes on ViewModel cleared`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onCleared()

        verify { chatRepository.unsubscribeFromChannel("ch-1") }
        verify { guildRepository.clearActiveChannel("ch-1") }
        verify { dmRepository.clearActiveChannel("ch-1") }
    }

    // ========================================================================
    // Additional: reactions
    // ========================================================================

    @Test
    fun `add reaction calls repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false
        coEvery { chatRepository.addReaction("ch-1", "msg-1", "thumbs_up") } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onAddReaction("msg-1", "thumbs_up")
        advanceUntilIdle()

        coVerify { chatRepository.addReaction("ch-1", "msg-1", "thumbs_up") }
    }

    @Test
    fun `remove reaction calls repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false
        coEvery { chatRepository.removeReaction("ch-1", "msg-1", "thumbs_up") } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onRemoveReaction("msg-1", "thumbs_up")
        advanceUntilIdle()

        coVerify { chatRepository.removeReaction("ch-1", "msg-1", "thumbs_up") }
    }

    // ========================================================================
    // Additional: load more (pagination)
    // ========================================================================

    @Test
    fun `load more calls repository with oldest message id`() = runTest {
        coEvery { chatRepository.loadMessages("ch-1", null) } coAnswers {
            messagesFlow.value = sampleMessages
            true
        }
        coEvery { chatRepository.loadMessages("ch-1", "msg-1") } returns false

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onLoadMore()
        advanceUntilIdle()

        coVerify { chatRepository.loadMessages("ch-1", "msg-1") }
    }

    @Test
    fun `canLoadMore follows hasMore from repository`() = runTest {
        coEvery { chatRepository.loadMessages("ch-1", null) } coAnswers {
            messagesFlow.value = sampleMessages
            true
        }
        coEvery { chatRepository.loadMessages("ch-1", "msg-1") } returns false

        viewModel = createViewModel()
        assertFalse(viewModel.canLoadMore.value)
        advanceUntilIdle()
        assertTrue(viewModel.canLoadMore.value)

        viewModel.onLoadMore()
        advanceUntilIdle()
        assertFalse(viewModel.canLoadMore.value)
    }

    @Test
    fun `load more does nothing when server has no older messages`() = runTest {
        coEvery { chatRepository.loadMessages("ch-1", null) } coAnswers {
            messagesFlow.value = sampleMessages
            false
        }

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onLoadMore()
        advanceUntilIdle()

        coVerify(exactly = 0) { chatRepository.loadMessages("ch-1", "msg-1") }
    }

    // ========================================================================
    // Additional: typing indicator
    // ========================================================================

    @Test
    fun `typing users are exposed from repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.typingUserIds.test {
            assertEquals(emptySet<String>(), awaitItem())

            // The signed-in user ("me") is never shown as typing.
            typingUsersFlow.value = mapOf("user-2" to 8_000L, "user-3" to 8_000L, "me" to 8_000L)
            assertEquals(setOf("user-2", "user-3"), awaitItem())
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `member names come from message authors`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false
        viewModel = createViewModel()

        viewModel.memberNames.test {
            assertEquals(emptyMap<String, String>(), awaitItem())
            messagesFlow.value = sampleMessages
            assertEquals(mapOf("user-1" to "Test User"), awaitItem())
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `input change triggers typing indicator`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onMessageInputChanged("typing...")
        advanceUntilIdle()

        verify { chatRepository.sendTypingIndicator("ch-1") }
    }

    // ========================================================================
    // Title
    // ========================================================================

    @Test
    fun `title is null for unknown channel and never the id`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false
        viewModel = createViewModel()

        viewModel.title.test {
            assertNull(awaitItem())
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `title resolves dm participant once conversations load`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false
        viewModel = createViewModel()

        viewModel.title.test {
            assertNull(awaitItem())
            conversationsFlow.value = listOf(
                DmConversation(
                    id = "ch-1",
                    participants = listOf(
                        DmParticipant(userId = "me", username = "me", displayName = "Me"),
                        DmParticipant(userId = "u2", username = "alice", displayName = "Alice"),
                    ),
                )
            )
            assertEquals(ChannelTitle("Alice", isDm = true), awaitItem())
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `title uses guild channel name`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } returns false
        channelsFlow.value = listOf(Channel(id = "ch-1", name = "general", channelType = ChannelType.TEXT))
        viewModel = createViewModel()

        viewModel.title.test {
            var item = awaitItem()
            if (item == null) item = awaitItem()
            assertEquals(ChannelTitle("general", isDm = false), item)
            cancelAndIgnoreRemainingEvents()
        }
    }

    // ========================================================================
    // Errors, editing, connection status
    // ========================================================================

    private fun loadedViewModel(): TextChannelViewModel {
        coEvery { chatRepository.loadMessages(any(), any()) } coAnswers {
            messagesFlow.value = sampleMessages
            false
        }
        return createViewModel()
    }

    @Test
    fun `send failure exposes error and keeps the message as not sent`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } throws RuntimeException("boom")

        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()

        assertEquals(R.string.chat_send_failed, viewModel.error.value)
        assertEquals("", viewModel.messageInput.value)
        val failed = viewModel.pendingMessages.value.single()
        assertEquals("hi", failed.text)
        assertEquals(io.wolftown.kaiku.data.repository.PendingMessage.State.FAILED, failed.state)

        viewModel.clearError()
        assertNull(viewModel.error.value)
    }

    @Test
    fun `send survives leaving the chat and a failure shows when coming back`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } coAnswers {
            gate.await()
            throw RuntimeException("boom")
        }

        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle() // send is now suspended on the gate
        leave(viewModel) // the user went back
        gate.complete(Unit)
        advanceUntilIdle()

        coVerify(exactly = 1) { chatRepository.sendMessage("ch-1", "hi", 1L) }
        val again = loadedViewModel()
        advanceUntilIdle()
        assertEquals("hi", again.pendingMessages.value.single().text)
        assertEquals(
            io.wolftown.kaiku.data.repository.PendingMessage.State.FAILED,
            again.pendingMessages.value.single().state,
        )
    }

    @Test
    fun `send that completes after leaving the chat is not cancelled`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } coAnswers { gate.await() }

        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()
        assertEquals(1, viewModel.pendingMessages.value.size)
        leave(viewModel)
        gate.complete(Unit)
        advanceUntilIdle()

        val again = loadedViewModel()
        advanceUntilIdle()
        assertTrue(again.pendingMessages.value.isEmpty())
        assertNull(again.error.value)
    }

    @Test
    fun `retry sends a failed message once even when tapped twice`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } throws RuntimeException("boom")
        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()
        val id = viewModel.pendingMessages.value.single().localId

        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } coAnswers { gate.await() }
        viewModel.onRetryPending(id)
        viewModel.onRetryPending(id)
        viewModel.onRetryFailed()
        advanceUntilIdle()
        gate.complete(Unit)
        advanceUntilIdle()

        coVerify(exactly = 2) { chatRepository.sendMessage("ch-1", "hi", 1L) } // first try + one retry
        assertTrue(viewModel.pendingMessages.value.isEmpty())
    }

    @Test
    fun `discarding a failed message removes it`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } throws RuntimeException("boom")
        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()

        viewModel.onDiscardPending(viewModel.pendingMessages.value.single().localId)
        advanceUntilIdle()

        assertTrue(viewModel.pendingMessages.value.isEmpty())
    }

    @Test
    fun `the failure notice names the failed message and ends when it is deleted`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        assertNull(viewModel.failureNotice.value)
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } throws RuntimeException("boom")
        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()
        val id = viewModel.pendingMessages.value.single().localId
        assertEquals(id, viewModel.failureNotice.value)

        viewModel.onDiscardPending(id)
        advanceUntilIdle()

        assertNull(viewModel.failureNotice.value)
    }

    @Test
    fun `the failure notice ends when the message is retried from its row and sent`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } throws RuntimeException("boom")
        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()
        val id = viewModel.pendingMessages.value.single().localId
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } coAnswers { gate.await() }

        viewModel.onRetryPending(id)
        advanceUntilIdle()
        assertNull(viewModel.failureNotice.value) // sending again: the notice no longer applies

        gate.complete(Unit)
        advanceUntilIdle()
        assertNull(viewModel.failureNotice.value)
        assertTrue(viewModel.pendingMessages.value.isEmpty())
    }

    @Test
    fun `a second failure of the retried message brings the notice back`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } throws RuntimeException("boom")
        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()
        val id = viewModel.pendingMessages.value.single().localId

        viewModel.onRetryPending(id)
        advanceUntilIdle()

        assertEquals(id, viewModel.failureNotice.value)
    }

    @Test
    fun `a failure event that arrives after the message was deleted brings no notice back`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.sendMessage("ch-1", "hi", 1L) } throws RuntimeException("boom")
        // Deletes the message right when its failure is emitted, before the view model sees the event.
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            outgoing.failures.collect { failed -> viewModel.onDiscardPending(failed.localId) }
        }
        viewModel.onMessageInputChanged("hi")
        viewModel.onSendMessage()
        advanceUntilIdle()

        assertTrue(viewModel.pendingMessages.value.isEmpty())
        assertNull(viewModel.failureNotice.value)
        assertNull(viewModel.error.value) // nothing left to report
    }

    @Test
    fun `only not-sent texts are tied to a failed message`() {
        assertTrue(isPendingFailureRes(R.string.chat_send_failed))
        assertTrue(isPendingFailureRes(R.string.chat_upload_failed))
        assertTrue(isPendingFailureRes(R.string.chat_upload_too_large))
        assertFalse(isPendingFailureRes(R.string.chat_edit_failed))
        assertFalse(isPendingFailureRes(R.string.chat_load_failed))
    }

    @Test
    fun `draft survives leaving the chat`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        viewModel.onMessageInputChanged("half a thought")
        leave(viewModel)

        savedStateHandle = SavedStateHandle(mapOf("channelId" to "ch-1"))
        val again = loadedViewModel()
        advanceUntilIdle()

        assertEquals("half a thought", again.messageInput.value)
    }

    @Test
    fun `a very long draft stays in memory but not in the saved state`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        val long = "x".repeat(TextChannelViewModel.MAX_SAVED_DRAFT_CHARS + 1)

        viewModel.onMessageInputChanged("short")
        assertEquals("short", savedStateHandle.get<String>("draft"))
        viewModel.onMessageInputChanged(long)

        assertNull(savedStateHandle.get<String>("draft"))
        assertEquals(long, viewModel.messageInput.value)
    }

    @Test
    fun `deleting a photo that was too large puts its caption back into the field`() = runTest {
        val uri = mockk<Uri>()
        coEvery { chatRepository.sendAttachment("ch-1", uri, "look at this", any()) } returns
            Result.failure(ChatRepository.AttachmentException.TooLarge())
        viewModel = loadedViewModel()
        advanceUntilIdle()
        viewModel.onMessageInputChanged("look at this")
        viewModel.onAttachmentPicked(uri)
        advanceUntilIdle()
        assertEquals("", viewModel.messageInput.value)

        viewModel.onDiscardPending(viewModel.pendingMessages.value.single().localId)
        advanceUntilIdle()

        assertEquals("look at this", viewModel.messageInput.value)
        assertTrue(viewModel.pendingMessages.value.isEmpty())
    }

    @Test
    fun `draft is restored from saved state after process death`() = runTest {
        savedStateHandle = SavedStateHandle(mapOf("channelId" to "ch-1", "draft" to "kept"))
        viewModel = loadedViewModel()
        advanceUntilIdle()

        assertEquals("kept", viewModel.messageInput.value)
    }

    @Test
    fun `reaching the top twice while loading starts one load`() = runTest {
        coEvery { chatRepository.loadMessages("ch-1", null) } coAnswers {
            messagesFlow.value = sampleMessages
            true
        }
        val gate = kotlinx.coroutines.CompletableDeferred<Boolean>()
        coEvery { chatRepository.loadMessages("ch-1", "msg-1") } coAnswers { gate.await() }
        viewModel = createViewModel()
        advanceUntilIdle()

        assertTrue(viewModel.onLoadMore())
        assertFalse(viewModel.onLoadMore())
        advanceUntilIdle()
        assertTrue(viewModel.isLoadingMore.value)
        gate.complete(false)
        advanceUntilIdle()

        assertFalse(viewModel.isLoadingMore.value)
        coVerify(exactly = 1) { chatRepository.loadMessages("ch-1", "msg-1") }
    }

    @Test
    fun `edit failure exposes error`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.editMessage("msg-1", "new") } throws RuntimeException("boom")

        viewModel.onEditMessage("msg-1", "new")
        advanceUntilIdle()

        assertEquals(R.string.chat_edit_failed, viewModel.error.value)
    }

    @Test
    fun `delete failure exposes error`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.deleteMessage("ch-1", "msg-1") } throws RuntimeException("boom")

        viewModel.onDeleteMessage("msg-1")
        advanceUntilIdle()

        assertEquals(R.string.chat_delete_failed, viewModel.error.value)
    }

    @Test
    fun `initial load failure exposes load error`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } throws RuntimeException("boom")
        viewModel = createViewModel()
        advanceUntilIdle()

        assertEquals(R.string.chat_load_failed, viewModel.error.value)
    }

    @Test
    fun `reaction failure exposes error`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.addReaction("ch-1", "msg-1", "x") } throws RuntimeException("boom")

        viewModel.onAddReaction("msg-1", "x")
        advanceUntilIdle()

        assertEquals(R.string.chat_reaction_failed, viewModel.error.value)
    }

    @Test
    fun `confirm edit calls repository and closes dialog`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        coEvery { chatRepository.editMessage("msg-1", "new") } just Runs

        viewModel.onStartEdit("msg-1")
        assertEquals("msg-1", viewModel.editingMessage.value?.id)

        viewModel.onConfirmEdit("new")
        advanceUntilIdle()

        coVerify { chatRepository.editMessage("msg-1", "new") }
        assertNull(viewModel.editingMessage.value)
    }

    @Test
    fun `blank edit is ignored`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()

        viewModel.onStartEdit("msg-1")
        viewModel.onConfirmEdit("   ")
        advanceUntilIdle()

        coVerify(exactly = 0) { chatRepository.editMessage(any(), any()) }
    }

    @Test
    fun `cancel edit closes dialog without calling repository`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()

        viewModel.onStartEdit("msg-1")
        viewModel.onCancelEdit()

        assertNull(viewModel.editingMessage.value)
        coVerify(exactly = 0) { chatRepository.editMessage(any(), any()) }
    }

    @Test
    fun `connection status mirrors realtime manager`() = runTest {
        viewModel = loadedViewModel()
        advanceUntilIdle()
        assertEquals(RealtimeStatus.Connected, viewModel.connectionStatus.value)

        statusFlow.value = RealtimeStatus.Reconnecting
        assertEquals(RealtimeStatus.Reconnecting, viewModel.connectionStatus.value)
    }

    @Test
    fun `upload failure exposes chat_upload_failed`() = runTest {
        val uri = mockk<Uri>()
        coEvery { chatRepository.sendAttachment("ch-1", uri, null, any()) } returns
            Result.failure(ChatRepository.AttachmentException.Failed())
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onAttachmentPicked(uri)
        advanceUntilIdle()

        assertEquals(R.string.chat_upload_failed, viewModel.error.value)
        assertFalse(viewModel.isUploading.value)
    }

    @Test
    fun `too large upload exposes chat_upload_too_large`() = runTest {
        val uri = mockk<Uri>()
        coEvery { chatRepository.sendAttachment("ch-1", uri, null, any()) } returns
            Result.failure(ChatRepository.AttachmentException.TooLarge())
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onAttachmentPicked(uri)
        advanceUntilIdle()

        assertEquals(R.string.chat_upload_too_large, viewModel.error.value)
    }

    @Test
    fun `unsupported type exposes chat_upload_unsupported_type`() = runTest {
        val uri = mockk<Uri>()
        coEvery { chatRepository.sendAttachment("ch-1", uri, null, any()) } returns
            Result.failure(ChatRepository.AttachmentException.UnsupportedType())
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onAttachmentPicked(uri)
        advanceUntilIdle()

        assertEquals(R.string.chat_upload_unsupported_type, viewModel.error.value)
    }

    @Test
    fun `caption is cleared during upload and kept with the failed upload`() = runTest {
        val uri = mockk<Uri>()
        val gate = kotlinx.coroutines.CompletableDeferred<Result<Message>>()
        coEvery { chatRepository.sendAttachment("ch-1", uri, "look", any()) } coAnswers { gate.await() }
        viewModel = createViewModel()
        advanceUntilIdle()
        viewModel.onMessageInputChanged("look")

        viewModel.onAttachmentPicked(uri)
        advanceUntilIdle()
        assertEquals("", viewModel.messageInput.value)
        assertTrue(viewModel.isUploading.value)

        leave(viewModel) // leaving the chat does not cancel the upload
        gate.complete(Result.failure(ChatRepository.AttachmentException.Failed()))
        advanceUntilIdle()
        val again = createViewModel()
        advanceUntilIdle()
        val failed = again.pendingMessages.value.single()
        assertEquals("look", failed.text)
        assertEquals(uri, failed.attachment)
        assertFalse(again.isUploading.value)
    }

    @Test
    fun `picks while uploading are ignored`() = runTest {
        val uri = mockk<Uri>()
        val gate = kotlinx.coroutines.CompletableDeferred<Result<Message>>()
        coEvery { chatRepository.sendAttachment("ch-1", any(), any(), any()) } coAnswers { gate.await() }
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onAttachmentPicked(uri)
        viewModel.onAttachmentPicked(uri)
        advanceUntilIdle()
        gate.complete(Result.success(sampleMessages[0]))
        advanceUntilIdle()

        coVerify(exactly = 1) { chatRepository.sendAttachment("ch-1", any(), any(), any()) }
    }

    @Test
    fun `successful upload sends draft as caption and keeps input empty`() = runTest {
        val uri = mockk<Uri>()
        coEvery { chatRepository.sendAttachment("ch-1", uri, "look", any()) } returns
            Result.success(sampleMessages[0])
        viewModel = createViewModel()
        advanceUntilIdle()
        viewModel.onMessageInputChanged("look")

        viewModel.onAttachmentPicked(uri)
        advanceUntilIdle()

        assertEquals("", viewModel.messageInput.value)
        assertNull(viewModel.error.value)
    }

    // ========================================================================
    // Notifications: visible screen clears and suppresses
    // ========================================================================

    @Test
    fun `visible screen clears notification and marks newest message seen`() = runTest {
        messagesFlow.value = sampleMessages
        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onVisible()
        advanceUntilIdle()

        assertEquals("ch-1", activeChannelTracker.visibleChannelId.value)
        verify { notificationSink.cancel("ch-1") }
        verify { notificationLedger.markSeen("me", "ch-1", "msg-2", "2026-03-12T10:01:00Z") }

        viewModel.onHidden()
        assertNull(activeChannelTracker.visibleChannelId.value)
    }

    // ========================================================================
    // Kaiku DM (C28): read-only, recognised by flags only, latest page on reconnect
    // ========================================================================

    @Test
    fun `kaiku dm hides the composer`() = runTest {
        conversationsFlow.value = listOf(
            io.wolftown.kaiku.domain.model.DmConversation("ch-1", name = "Kaiku", isSystem = true, readOnly = true),
        )
        val vm = createViewModel()
        advanceUntilIdle()
        assertTrue(vm.isKaikuDm.value)
    }

    @Test
    fun `kaiku dm is recognised by flags only`() = runTest {
        conversationsFlow.value = listOf(
            io.wolftown.kaiku.domain.model.DmConversation(
                "ch-1", name = "Kaiku",
                participants = listOf(io.wolftown.kaiku.domain.model.DmParticipant("6b61696b-7500-4000-8000-000000000001", "kaiku", "Kaiku")),
            ),
        )
        val vm = createViewModel()
        advanceUntilIdle()
        assertFalse(vm.isKaikuDm.value)
    }

    @Test
    fun `kaiku dm reloads the latest page on reconnect`() = runTest {
        conversationsFlow.value = listOf(
            io.wolftown.kaiku.domain.model.DmConversation("ch-1", isSystem = true, readOnly = true),
        )
        val vm = createViewModel()
        advanceUntilIdle()
        statusFlow.value = RealtimeStatus.Reconnecting
        advanceUntilIdle()
        statusFlow.value = RealtimeStatus.Connected
        advanceUntilIdle()
        assertTrue(vm.isKaikuDm.value)
        io.mockk.coVerify(exactly = 2) { chatRepository.loadMessages("ch-1", null) }
    }

    @org.junit.Test
    fun `a security notice card only for the system account in the Kaiku DM`() {
        val system = io.wolftown.kaiku.domain.model.User("k", "kaiku", "Kaiku", isSystem = true)
        val person = io.wolftown.kaiku.domain.model.User("u2", "mallory", "Kaiku")
        val notice = io.mockk.mockk<io.wolftown.kaiku.domain.model.Message>(relaxed = true) {
            io.mockk.every { messageType } returns "security"
            io.mockk.every { author } returns system
        }
        org.junit.Assert.assertTrue(io.wolftown.kaiku.ui.channel.isSecurityNotice(true, notice))
        org.junit.Assert.assertFalse(io.wolftown.kaiku.ui.channel.isSecurityNotice(false, notice))
        io.mockk.every { notice.author } returns person
        org.junit.Assert.assertFalse(io.wolftown.kaiku.ui.channel.isSecurityNotice(true, notice))
    }
}
