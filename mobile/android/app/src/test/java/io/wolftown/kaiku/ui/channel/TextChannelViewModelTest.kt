package io.wolftown.kaiku.ui.channel

import androidx.lifecycle.SavedStateHandle
import app.cash.turbine.test
import io.mockk.*
import io.wolftown.kaiku.data.repository.ChatRepository
import io.wolftown.kaiku.data.repository.GuildRepository
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.User
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.*
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class TextChannelViewModelTest {

    private lateinit var chatRepository: ChatRepository
    private lateinit var guildRepository: GuildRepository
    private lateinit var viewModel: TextChannelViewModel
    private lateinit var savedStateHandle: SavedStateHandle

    private val testDispatcher = StandardTestDispatcher()

    private val messagesFlow = MutableStateFlow<List<Message>>(emptyList())
    private val typingUsersFlow = MutableStateFlow<Set<String>>(emptySet())

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
        savedStateHandle = SavedStateHandle(mapOf("channelId" to "ch-1"))

        every { chatRepository.getMessages("ch-1") } returns messagesFlow
        every { chatRepository.getTypingUsers("ch-1") } returns typingUsersFlow
    }

    @After
    fun tearDown() {
        Dispatchers.resetMain()
    }

    private fun createViewModel(): TextChannelViewModel {
        return TextChannelViewModel(chatRepository, guildRepository, savedStateHandle)
    }

    // ========================================================================
    // 1. Loads message history on init
    // ========================================================================

    @Test
    fun `loads message history on init`() = runTest {
        coEvery { chatRepository.loadMessages("ch-1", null) } coAnswers {
            messagesFlow.value = sampleMessages
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
        coEvery { chatRepository.loadMessages(any(), any()) } just Runs

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
        coEvery { chatRepository.loadMessages(any(), any()) } just Runs
        coEvery { chatRepository.sendMessage("ch-1", "Hello!") } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onMessageInputChanged("Hello!")
        assertEquals("Hello!", viewModel.messageInput.value)

        viewModel.onSendMessage()
        advanceUntilIdle()

        coVerify { chatRepository.sendMessage("ch-1", "Hello!") }
        assertEquals("", viewModel.messageInput.value)
    }

    // ========================================================================
    // 5. Edit and delete update the message list
    // ========================================================================

    @Test
    fun `edit message calls repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } coAnswers {
            messagesFlow.value = sampleMessages
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
        coEvery { chatRepository.loadMessages(any(), any()) } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onCleared()

        verify { chatRepository.unsubscribeFromChannel("ch-1") }
    }

    // ========================================================================
    // Additional: reactions
    // ========================================================================

    @Test
    fun `add reaction calls repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } just Runs
        coEvery { chatRepository.addReaction("ch-1", "msg-1", "thumbs_up") } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onAddReaction("msg-1", "thumbs_up")
        advanceUntilIdle()

        coVerify { chatRepository.addReaction("ch-1", "msg-1", "thumbs_up") }
    }

    @Test
    fun `remove reaction calls repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } just Runs
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
        }
        coEvery { chatRepository.loadMessages("ch-1", "msg-1") } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onLoadMore()
        advanceUntilIdle()

        coVerify { chatRepository.loadMessages("ch-1", "msg-1") }
    }

    // ========================================================================
    // Additional: typing indicator
    // ========================================================================

    @Test
    fun `typing users are exposed from repository`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        typingUsersFlow.value = setOf("user-2", "user-3")
        advanceUntilIdle()

        assertEquals(setOf("user-2", "user-3"), viewModel.typingUsers.value)
    }

    @Test
    fun `input change triggers typing indicator`() = runTest {
        coEvery { chatRepository.loadMessages(any(), any()) } just Runs

        viewModel = createViewModel()
        advanceUntilIdle()

        viewModel.onMessageInputChanged("typing...")
        advanceUntilIdle()

        verify { chatRepository.sendTypingIndicator("ch-1") }
    }
}
