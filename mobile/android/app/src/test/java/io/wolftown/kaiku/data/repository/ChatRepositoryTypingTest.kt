package io.wolftown.kaiku.data.repository

import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.KaikuJson
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.data.ws.WallClock
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.currentTime
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class ChatRepositoryTypingTest {

    private val eventsFlow = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 64)

    private fun kotlinx.coroutines.test.TestScope.repository(): ChatRepository {
        val webSocket = mockk<KaikuWebSocket>(relaxed = true)
        every { webSocket.events } returns eventsFlow
        val authState = mockk<AuthState>(relaxed = true)
        every { authState.currentUserId } returns MutableStateFlow<String?>("me")
        return ChatRepository(
            mockk<MessageApi>(relaxed = true),
            webSocket,
            KaikuJson,
            mockk<GuildRepository>(relaxed = true),
            authState,
            CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher(testScheduler)),
            WallClock { currentTime },
        )
    }

    @Test
    fun `typing expires after 8 s`() = runTest {
        val repo = repository()
        val typing = repo.getTypingUsers("c1")

        eventsFlow.emit(ServerEvent.TypingStart("c1", "u1"))
        assertEquals(setOf("u1"), typing.value.keys)

        advanceTimeBy(7_999)
        assertEquals(setOf("u1"), typing.value.keys)

        advanceTimeBy(2)
        assertTrue(typing.value.isEmpty())
    }

    @Test
    fun `a new typing event extends the expiry`() = runTest {
        val repo = repository()
        val typing = repo.getTypingUsers("c1")

        eventsFlow.emit(ServerEvent.TypingStart("c1", "u1"))
        advanceTimeBy(5_000)
        eventsFlow.emit(ServerEvent.TypingStart("c1", "u1"))
        assertEquals(13_000L, typing.value["u1"])

        advanceTimeBy(5_000) // 10 s after the first event, 5 s after the second
        assertEquals(setOf("u1"), typing.value.keys)

        advanceTimeBy(3_001)
        assertTrue(typing.value.isEmpty())
    }

    @Test
    fun `typing stop removes the user at once`() = runTest {
        val repo = repository()
        val typing = repo.getTypingUsers("c1")

        eventsFlow.emit(ServerEvent.TypingStart("c1", "u1"))
        eventsFlow.emit(ServerEvent.TypingStop("c1", "u1"))

        assertTrue(typing.value.isEmpty())
    }
}
