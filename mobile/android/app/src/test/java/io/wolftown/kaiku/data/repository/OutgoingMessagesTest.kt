package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.slot
import io.wolftown.kaiku.data.api.SessionChangedException
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.coroutines.cancellation.CancellationException

/** AND-B-3: sending in the app's scope, in order, once, and never across sessions. */
@OptIn(ExperimentalCoroutinesApi::class)
class OutgoingMessagesTest {

    private val chat = mockk<ChatRepository>(relaxed = true)
    private val clearListener = slot<() -> Unit>()
    private var ids = 0

    init {
        every { chat.addSessionClearListener(capture(clearListener)) } returns Unit
    }

    /** Real [AuthState] (session ids, ended sessions), driven by the test scheduler. */
    private fun TestScope.auth(): AuthState {
        val auth = AuthState(CoroutineScope(SupervisorJob() + StandardTestDispatcher(testScheduler)))
        auth.setLoggedIn("anna")
        return auth
    }

    private fun TestScope.outgoing(auth: AuthState) = OutgoingMessages(
        chat, auth, CoroutineScope(SupervisorJob() + StandardTestDispatcher(testScheduler)),
    ) { "l${ids++}" }

    private fun AuthState.sessionId() = (session.value as AuthSession.LoggedIn).sessionId

    @Test
    fun `messages of one chat go out in the order they were sent`() = runTest {
        val sent = mutableListOf<String>()
        val first = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", any(), any()) } coAnswers {
            val text = secondArg<String>()
            if (text == "one") first.await()
            sent += text
        }
        val out = outgoing(auth())

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
    fun `every send is bound to the session it was sent in`() = runTest {
        val auth = auth()
        val out = outgoing(auth)

        out.send("c1", "hi")
        advanceUntilIdle()

        coVerify(exactly = 1) { chat.sendMessage("c1", "hi", auth.sessionId()) }
    }

    @Test
    fun `sign out and in again as the same account while a send waits sends nothing of the old session`() =
        runTest {
            val gate = CompletableDeferred<Unit>()
            coEvery { chat.sendMessage("c1", "one", any()) } coAnswers { gate.await() }
            val auth = auth()
            val first = auth.sessionId()
            val out = outgoing(auth)
            out.setDraft("c1", "draft")
            out.send("c1", "one")
            out.send("c1", "two") // waits behind "one"
            advanceUntilIdle()

            // Both transitions before any collector runs (no conflation gap).
            auth.setLoggedOut()
            auth.setLoggedIn("anna")
            gate.complete(Unit)
            advanceUntilIdle()

            coVerify(exactly = 0) { chat.sendMessage("c1", "two", any()) }
            coVerify(exactly = 0) { chat.sendMessage(any(), any(), auth.sessionId()) }
            coVerify(exactly = 1) { chat.sendMessage("c1", "one", first) }
            assertTrue(out.pendingNow("c1").isEmpty())
            assertEquals("", out.draft("c1"))
        }

    @Test
    fun `switching straight to another account never sends the old account's message`() = runTest {
        val gate = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", "one", any()) } coAnswers { gate.await() }
        val auth = auth()
        val out = outgoing(auth)
        out.send("c1", "one")
        out.send("c1", "two")
        advanceUntilIdle()

        auth.setLoggedIn("bert") // A -> B without LoggedOut in between
        gate.complete(Unit)
        advanceUntilIdle()

        coVerify(exactly = 0) { chat.sendMessage("c1", "two", any()) }
        assertTrue(out.pendingNow("c1").isEmpty())
    }

    @Test
    fun `a send that waits is checked against the session right before it goes out`() = runTest {
        val gate = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", "one", any()) } coAnswers { gate.await() }
        val auth = auth()
        val out = outgoing(auth)
        out.send("c1", "one")
        out.send("c1", "two")
        advanceUntilIdle()

        // Session changes but no observer has run yet: the pre-send check alone must hold.
        auth.setLoggedIn("bert")
        gate.complete(Unit)
        testScheduler.runCurrent()
        advanceUntilIdle()

        coVerify(exactly = 0) { chat.sendMessage("c1", "two", any()) }
    }

    @Test
    fun `session teardown clears explicitly through the chat cache clear`() = runTest {
        val gate = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", "one", any()) } coAnswers { gate.await() }
        val auth = auth()
        val out = outgoing(auth)
        out.setDraft("c1", "draft")
        out.send("c1", "one")
        advanceUntilIdle()

        auth.setLoggedOut()
        clearListener.captured() // CacheSessionTeardown -> ChatRepository.clearSession()

        assertTrue(out.pendingNow("c1").isEmpty())
        assertEquals("", out.draft("c1"))
    }

    @Test
    fun `a request refused because the session changed is dropped, not failed`() = runTest {
        coEvery { chat.sendMessage("c1", "hi", any()) } throws SessionChangedException()
        val out = outgoing(auth())

        out.send("c1", "hi")
        advanceUntilIdle()

        assertTrue(out.pendingNow("c1").isEmpty())
    }

    @Test
    fun `a real cancellation of the send drops it`() = runTest {
        coEvery { chat.sendAttachment("c1", any(), any(), any()) } coAnswers {
            currentCoroutineContext()[Job]!!.cancel() // what ChatRepository.clearSession does
            throw CancellationException("session cleared")
        }
        val out = outgoing(auth())

        out.sendAttachment("c1", mockk(), "look")
        advanceUntilIdle()

        assertTrue(out.pendingNow("c1").isEmpty())
    }

    @Test
    fun `a stray cancellation exception from below shows Not sent with Retry`() = runTest {
        coEvery { chat.sendAttachment("c1", any(), any(), any()) } throws CancellationException("timeout")
        val out = outgoing(auth())

        out.sendAttachment("c1", mockk(), "look")
        advanceUntilIdle()

        val failed = out.pendingNow("c1").single()
        assertEquals(PendingMessage.State.FAILED, failed.state)
        assertEquals(PendingMessage.Failure.UPLOAD_FAILED, failed.failure)
    }

    @Test
    fun `a retry that fails again stays failed and can be retried`() = runTest {
        coEvery { chat.sendMessage("c1", "hi", any()) } throws RuntimeException("offline")
        val out = outgoing(auth())
        val id = out.send("c1", "hi")
        advanceUntilIdle()

        out.retry(id)
        advanceUntilIdle()
        assertEquals(PendingMessage.State.FAILED, out.pendingNow("c1").single().state)

        coEvery { chat.sendMessage("c1", "hi", any()) } returns Unit
        out.retry(id)
        advanceUntilIdle()
        assertTrue(out.pendingNow("c1").isEmpty())
        coVerify(exactly = 3) { chat.sendMessage("c1", "hi", any()) }
    }

    @Test
    fun `failed upload keeps its reason`() = runTest {
        coEvery { chat.sendAttachment("c1", any(), any(), any()) } returns
            Result.failure(ChatRepository.AttachmentException.TooLarge())
        val out = outgoing(auth())

        out.sendAttachment("c1", mockk(), null)
        advanceUntilIdle()

        val failed = out.pendingNow("c1").single()
        assertEquals(PendingMessage.State.FAILED, failed.state)
        assertEquals(PendingMessage.Failure.TOO_LARGE, failed.failure)
    }

    @Test
    fun `discard leaves a message that is still sending alone`() = runTest {
        val gate = CompletableDeferred<Unit>()
        coEvery { chat.sendMessage("c1", "hi", any()) } coAnswers { gate.await() }
        val out = outgoing(auth())
        val id = out.send("c1", "hi")
        advanceUntilIdle()

        assertNull(out.discard(id))
        assertEquals(1, out.pendingNow("c1").size)
        gate.complete(Unit)
        advanceUntilIdle()
        assertTrue(out.pendingNow("c1").isEmpty())
    }
}
