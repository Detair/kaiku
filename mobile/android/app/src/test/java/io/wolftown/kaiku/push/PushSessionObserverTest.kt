package io.wolftown.kaiku.push

import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.data.local.AuthState
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class PushSessionObserverTest {

    private val sink: NotificationSink = mockk(relaxed = true)
    private val enqueuer = FakeWakeWorkEnqueuer()

    @Test
    fun `logout clears notifications and wake work`() = runTest {
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, sink, enqueuer, backgroundScope).start()

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
        PushSessionObserver(authState, sink, enqueuer, backgroundScope).start()

        authState.setLoggedIn("alice")
        runCurrent()
        authState.setLoggedIn("bob")
        runCurrent()

        verify(exactly = 1) { sink.cancelAll() }
        assertEquals(1, enqueuer.cancelAllCount)
    }

    @Test
    fun `starting signed out clears nothing`() = runTest {
        val authState = AuthState(backgroundScope)
        PushSessionObserver(authState, sink, enqueuer, backgroundScope).start()

        authState.setLoggedOut()
        runCurrent()

        verify(exactly = 0) { sink.cancelAll() }
        assertEquals(0, enqueuer.cancelAllCount)
    }
}
