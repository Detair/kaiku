package io.wolftown.kaiku.push

import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class PushSessionObserverTest {

    private val sink: NotificationSink = mockk(relaxed = true)
    private val enqueuer = FakeWakeWorkEnqueuer()
    private val callStore = PrefsCallNotificationStore(InMemorySharedPreferences())
    private var storedRefreshToken: String? = "refresh"
    private val tokenStorage: TokenStorage = mockk {
        every { getRefreshToken() } answers { storedRefreshToken }
    }

    @Test
    fun `logout clears notifications and wake work`() = runTest {
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()

        authState.setLoggedIn("alice")
        runCurrent()
        verify(exactly = 0) { sink.cancelAll() }

        authState.setLoggedOut()
        runCurrent()
        verify(exactly = 1) { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }

    @Test
    fun `account change clears notifications and wake work`() = runTest {
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()

        authState.setLoggedIn("alice")
        runCurrent()
        authState.setLoggedIn("bob")
        runCurrent()

        verify(exactly = 1) { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }

    @Test
    fun `cold start without a stored session clears leftovers`() = runTest {
        storedRefreshToken = null
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()
        runCurrent()

        verify(exactly = 1) { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }

    @Test
    fun `starting signed out with a stored session clears nothing`() = runTest {
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()

        authState.setLoggedOut()
        runCurrent()

        verify(exactly = 0) { sink.cancelAll() }
        assertEquals(0, enqueuer.cancelAllCount)
    }

    @Test
    fun `logout forgets the departing user's calls only`() = runTest {
        callStore.setShown("alice", "c1", ShownCall("call-1", CallPhase.RINGING))
        callStore.setShown("alice", "c2", ShownCall("call-2", CallPhase.MISSED))
        callStore.setShown("bob", "c1", ShownCall("call-3", CallPhase.RINGING))
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()

        authState.setLoggedIn("alice")
        runCurrent()
        assertNotNull(callStore.shown("alice", "c1"))

        authState.setLoggedOut()
        runCurrent()

        assertNull(callStore.shown("alice", "c1"))
        assertNull(callStore.shown("alice", "c2"))
        assertEquals(ShownCall("call-3", CallPhase.RINGING), callStore.shown("bob", "c1"))
    }

    @Test
    fun `account change forgets the previous user's calls`() = runTest {
        callStore.setShown("alice", "c1", ShownCall("call-1", CallPhase.RINGING))
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()

        authState.setLoggedIn("alice")
        runCurrent()
        authState.setLoggedIn("bob")
        runCurrent()

        assertNull(callStore.shown("alice", "c1"))
    }

    @Test
    fun `cold start without a stored session forgets every call`() = runTest {
        storedRefreshToken = null
        callStore.setShown("alice", "c1", ShownCall("call-1", CallPhase.RINGING))
        callStore.setShown("bob", "c1", ShownCall("call-3", CallPhase.MISSED))
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()
        runCurrent()

        assertNull(callStore.shown("alice", "c1"))
        assertNull(callStore.shown("bob", "c1"))
    }

    @Test
    fun `starting signed out with a stored session keeps calls`() = runTest {
        callStore.setShown("alice", "c1", ShownCall("call-1", CallPhase.RINGING))
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, tokenStorage, sink, enqueuer, callStore, backgroundScope).start()

        authState.setLoggedOut()
        runCurrent()

        assertNotNull(callStore.shown("alice", "c1"))
    }
}
