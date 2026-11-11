package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.local.AuthState
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.coroutines.cancellation.CancellationException

/** AND-B-3: sending in the app's scope, in order, once, and never across sessions. */
@OptIn(ExperimentalCoroutinesApi::class)
class OutgoingMessagesTest {

    private val chat = mockk<ChatRepository>(relaxed = true)
    private val user = MutableStateFlow<String?>("me")
    private val auth = mockk<AuthState> { every { currentUserId } returns user }
    private var ids = 0

    private fun TestScope.outgoing() = OutgoingMessages(
        chat, auth, CoroutineScope(SupervisorJob() + StandardTestDispatcher(testScheduler)),
    ) { "l${ids++}" }

    @Test
    fun `messages of one chat go out in the order they were sent`() = runTest {
        val sent = mutableListOf<String>()
        val first = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", any()) } coAnswers {
            val text = secondArg<String>()
            if (text == "one") first.await()
            sent += text
        }
        val out = outgoing()

        out.send("c1", "one")
        out.send("c1", "two")
        advanceUntilIdle()
        assertEquals(emptyList<String>(), sent) // "two" waits for "one"
        first.complete(Unit)
        advanceUntilIdle()

        assertEquals(listOf("one", "two"), sent)
        assertTrue(out.pendingNow("c1").isEmpty())
    }

    @Test
    fun `signing out cancels sends and drops pending messages and drafts`() = runTest {
        val gate = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", "hi") } coAnswers { gate.await() }
        val out = outgoing()
        out.setDraft("c1", "draft")
        out.send("c1", "hi")
        advanceUntilIdle()
        assertEquals(1, out.pendingNow("c1").size)

        user.value = null
        advanceUntilIdle()

        assertTrue(out.pendingNow("c1").isEmpty())
        assertEquals("", out.draft("c1"))
        gate.complete(Unit) // a late answer changes nothing
        advanceUntilIdle()
        assertTrue(out.pendingNow("c1").isEmpty())
    }

    @Test
    fun `an upload cancelled by the session teardown disappears`() = runTest {
        coEvery { chat.sendAttachment("c1", any(), any()) } throws CancellationException("session cleared")
        val out = outgoing()

        out.sendAttachment("c1", mockk(), "look")
        advanceUntilIdle()

        assertTrue(out.pendingNow("c1").isEmpty())
    }

    @Test
    fun `failed upload keeps its reason and a refused type is not retried blindly`() = runTest {
        coEvery { chat.sendAttachment("c1", any(), any()) } returns
            Result.failure(ChatRepository.AttachmentException.TooLarge())
        val out = outgoing()

        out.sendAttachment("c1", mockk(), null)
        advanceUntilIdle()

        val failed = out.pendingNow("c1").single()
        assertEquals(PendingMessage.State.FAILED, failed.state)
        assertEquals(PendingMessage.Failure.TOO_LARGE, failed.failure)
        coVerify(exactly = 1) { chat.sendAttachment("c1", any(), null) }
    }

    @Test
    fun `discard leaves a message that is still sending alone`() = runTest {
        val gate = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", "hi") } coAnswers { gate.await() }
        val out = outgoing()
        val id = out.send("c1", "hi")
        advanceUntilIdle()

        out.discard(id)
        assertEquals(1, out.pendingNow("c1").size)
        gate.complete(Unit)
        advanceUntilIdle()
        assertTrue(out.pendingNow("c1").isEmpty())
    }
}
