package io.wolftown.kaiku.data.session

import io.wolftown.kaiku.data.api.SessionChangedException
import io.wolftown.kaiku.data.local.AuthSession
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/** The session generation: what belongs to a signed-in account ends with the hook's advance. */
@OptIn(ExperimentalCoroutinesApi::class)
class SessionGenerationTest {

    @After
    fun tearDown() {
        SessionEndHook.installFirst(null)
        SessionEndHook.install()
    }

    @Test
    fun `a ticket is for the signed-in account and for nobody else`() {
        val sessions = FakeSessions("anna")
        val ticket = sessions.generation.enter()!!
        assertEquals("anna", ticket.userId)
        assertTrue(ticket.alive)
        assertEquals(ticket, sessions.generation.enter("anna"))
        assertNull(sessions.generation.enter("bert"))
    }

    @Test
    fun `nobody signed in gets no ticket`() {
        assertNull(FakeSessions(user = null).generation.enter())
    }

    @Test
    fun `the advance ends every ticket at once, before the account state follows`() {
        val sessions = FakeSessions("anna")
        val ticket = sessions.generation.enter()!!
        sessions.generation.advance()
        // The auth state still names the old session: it is over all the same.
        assertTrue(sessions.session is AuthSession.LoggedIn)
        assertFalse(ticket.alive)
        assertFalse(ticket.open)
        assertNull("no new work for the session that is ending", sessions.generation.enter())
        assertThrows(SessionChangedException::class.java) { ticket.requestSession() }
    }

    @Test
    fun `the next account gets a new ticket and the old one stays dead`() {
        val sessions = FakeSessions("anna")
        val old = sessions.generation.enter()!!
        sessions.switchTo("bert")
        val next = sessions.generation.enter()!!
        assertEquals("bert", next.userId)
        assertNotEquals(old, next)
        assertFalse(old.alive)
        assertTrue(next.alive)
        assertEquals(sessions.sessionId, next.requestSession())
    }

    @Test
    fun `the same account signing in again is a new generation too`() {
        val sessions = FakeSessions("anna")
        val old = sessions.generation.enter()!!
        sessions.switchTo("anna")
        assertFalse(old.alive)
        assertNotEquals(old, sessions.generation.enter())
    }

    @Test
    fun `a scope of a generation is cancelled by the advance, synchronously`() = runTest {
        val sessions = FakeSessions("anna")
        val ticket = sessions.generation.enter()!!
        val scope = ticket.scope(backgroundScope)
        var ran = false
        val job = scope.launch {
            ran = true
            awaitCancellation()
        }
        runCurrent()
        assertTrue(ran)
        sessions.generation.advance()
        // No dispatch needed: cancelled when advance returns.
        assertTrue(job.isCancelled)
        assertFalse(scope.isActive)
    }

    @Test
    fun `a scope asked for after the advance never runs anything`() = runTest {
        val sessions = FakeSessions("anna")
        val ticket = sessions.generation.enter()!!
        sessions.generation.advance()
        var ran = false
        ticket.scope(backgroundScope).launch { ran = true }
        runCurrent()
        assertFalse(ran)
    }

    @Test
    fun `resets run inside the advance and a failing one does not stop the others`() {
        val sessions = FakeSessions("anna")
        val log = mutableListOf<String>()
        sessions.generation.onAdvance { throw IllegalStateException("boom") }
        sessions.generation.onAdvance { log += "cleared at ${sessions.generation.current}" }
        val before = sessions.generation.current
        sessions.generation.advance()
        assertEquals(listOf("cleared at ${before + 1}"), log)
    }

    @Test
    fun `a session that is being signed out takes no new work but keeps what runs`() {
        val sessions = FakeSessions("anna")
        val running = sessions.generation.enter()!!
        sessions.generation.close(sessions.sessionId)
        assertTrue("what runs goes on until the advance", running.alive)
        assertFalse(running.open)
        assertFalse(sessions.generation.enter()!!.open)
        // The next session is open again.
        sessions.switchTo("anna")
        assertTrue(sessions.generation.enter()!!.open)
    }

    @Test
    fun `a process that has not restored its session works for the stored account`() {
        val sessions = FakeSessions(user = null)
        sessions.stored = "anna"
        val ticket = sessions.generation.enter("anna")!!
        assertTrue(ticket.alive)
        // No restored session: a request cannot be bound, so it is not sent.
        assertThrows(SessionChangedException::class.java) { ticket.requestSession() }
        // Restoring the session (the app opens) keeps the same ticket.
        sessions.signIn("anna")
        assertTrue(ticket.alive)
        assertEquals(ticket, sessions.generation.enter())
        // The stored session is removed: over.
        sessions.signOut()
        assertFalse(ticket.alive)
    }

    @Test
    fun `the hook advances the generation before any other part runs`() {
        val sessions = FakeSessions("anna")
        val ticket = sessions.generation.enter()!!
        val seen = mutableListOf<Boolean>()
        // Installed in the "wrong" order on purpose.
        SessionEndHook.install({ seen += ticket.alive }, { seen += ticket.alive })
        SessionEndHook.installFirst { sessions.generation.advance() }
        SessionEndHook.beforeSessionEnds()
        assertEquals("every part already sees the old generation as over", listOf(false, false), seen)
    }
}
