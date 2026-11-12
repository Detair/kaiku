package io.wolftown.kaiku.ui.channel

import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SnackbarUntilTest {

    @Test
    fun `an obsolete snackbar that is still queued never shows and leaves the current one alone`() = runTest {
        val host = SnackbarHostState()
        launch { host.showSnackbar("Saved to your phone.") }
        runCurrent()
        val obsolete = CompletableDeferred<Unit>()
        val failure = async { host.showSnackbarUntil("Couldn't send your message.", "Retry") { obsolete.await() } }
        runCurrent()
        assertEquals("Saved to your phone.", host.currentSnackbarData?.visuals?.message)

        obsolete.complete(Unit)
        runCurrent()

        assertNull(failure.await())
        assertEquals("Saved to your phone.", host.currentSnackbarData?.visuals?.message)
        host.currentSnackbarData?.dismiss()
        runCurrent()
        assertNull(host.currentSnackbarData) // the obsolete one does not appear afterwards
    }

    @Test
    fun `an obsolete snackbar that is showing goes away`() = runTest {
        val host = SnackbarHostState()
        val obsolete = CompletableDeferred<Unit>()
        val failure = async { host.showSnackbarUntil("Couldn't send your message.", "Retry") { obsolete.await() } }
        runCurrent()
        assertEquals("Couldn't send your message.", host.currentSnackbarData?.visuals?.message)

        obsolete.complete(Unit)
        runCurrent()

        assertNull(failure.await())
        assertNull(host.currentSnackbarData)
    }

    @Test
    fun `the action result comes back while the snackbar is still current`() = runTest {
        val host = SnackbarHostState()
        val failure = async { host.showSnackbarUntil("Couldn't send your message.", "Retry") { CompletableDeferred<Unit>().await() } }
        runCurrent()
        host.currentSnackbarData!!.performAction()
        assertEquals(SnackbarResult.ActionPerformed, failure.await())
    }

    @Test
    fun `an interrupted scroll does not end the caller`() = runTest {
        var after = false
        val job = launch {
            ignoringInterruption { throw kotlinx.coroutines.CancellationException("interrupted by a touch") }
            after = true
        }
        job.join()
        assertTrue(after)
    }

    @Test
    fun `a real cancellation still ends the caller`() = runTest {
        var after = false
        val job = launch {
            ignoringInterruption { CompletableDeferred<Unit>().await() }
            after = true
        }
        runCurrent()
        job.cancel()
        job.join()
        assertFalse(after)
    }
}
