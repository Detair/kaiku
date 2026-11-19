package io.wolftown.kaiku.data.auth

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.testutil.InMemorySharedPreferences
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The start-up handling of a sign-in that was made only to remove this phone. */
@OptIn(ExperimentalCoroutinesApi::class)
class UnfinishedRemovalTest {
    private val authApi: AuthApi = mockk(relaxed = true)
    private val running = RemovalsInProgress()

    private class Phone(val f: KeyDeviceFixture, val tokens: TokenStorage, val removal: UnfinishedRemoval)

    /** A key phone whose removal sign-in was stored (tokens and mark), as at the next app start. */
    private fun TestScope.phone(marked: Boolean = true): Phone {
        val f = KeyDeviceFixture(backgroundScope)
        f.authState.setLoggedOut() // a fresh process: nothing read yet
        val tokens = TokenStorage(InMemorySharedPreferences()).apply {
            saveServerUrl(SigFixtures.ORIGIN)
            saveTokens("access-r", "refresh-r", 900, SigFixtures.USER_ID)
        }
        if (marked) f.repo.markRemovalSignIn(true)
        return Phone(f, tokens, UnfinishedRemoval(f.repo, tokens, f.authState, authApi, running, backgroundScope))
    }

    @Test fun `a start after a killed removal clears the session before it is read`() = runTest {
        val p = phone()
        assertEquals(RemovalAtStart.ENDED, p.removal.atActivityStart())
        // Synchronously, before the auth state is set up from storage.
        assertNull(p.tokens.getAccessToken())
        assertNull(p.tokens.getRefreshToken())
        assertFalse(p.f.repo.removalSignInPending())
        p.f.authState.initialize(p.tokens)
        assertEquals(AuthSession.LoggedOut, p.f.authState.session.value)
        assertTrue(p.removal.startsAtConfirm(RemovalAtStart.ENDED)) // the key stays: "Confirm it's you"
        assertEquals(SigFixtures.ORIGIN, p.tokens.getServerUrl())
        // Only the best-effort server sign-out runs afterwards, with the session's own tokens.
        runCurrent()
        coVerify(exactly = 1) { authApi.logoutSession(SigFixtures.ORIGIN, "access-r", "refresh-r") }
    }

    @Test fun `a rotation during that start finds nothing left to do`() = runTest {
        val p = phone()
        coEvery { authApi.logoutSession(any(), any(), any()) } coAnswers { kotlinx.coroutines.awaitCancellation() }
        assertEquals(RemovalAtStart.ENDED, p.removal.atActivityStart())
        // The activity is created again while the server sign-out still waits.
        assertEquals(RemovalAtStart.NONE, p.removal.atActivityStart())
        p.f.authState.initialize(p.tokens)
        assertEquals(AuthSession.LoggedOut, p.f.authState.session.value)
    }

    @Test fun `every activity start checks again, not only the first one of the process`() = runTest {
        val p = phone(marked = false)
        assertEquals(RemovalAtStart.NONE, p.removal.atActivityStart())
        assertEquals("access-r", p.tokens.getAccessToken())
        // Later in the same process a removal sign-in is left behind (its screen went away).
        p.f.authState.setLoggedIn(SigFixtures.USER_ID)
        p.f.repo.markRemovalSignIn(true)
        assertEquals(RemovalAtStart.ENDED, p.removal.atActivityStart())
        assertEquals(AuthSession.LoggedOut, p.f.authState.session.value)
        assertNull(p.tokens.getAccessToken())
    }

    @Test fun `a removal still running in this process is left to its own clean-up`() = runTest {
        val p = phone()
        val lease = running.begin()
        assertEquals(RemovalAtStart.RUNNING, p.removal.atActivityStart())
        assertEquals("access-r", p.tokens.getAccessToken())
        assertTrue(p.f.repo.removalSignInPending())
        assertTrue(p.removal.startsAtConfirm(RemovalAtStart.RUNNING)) // never the signed-in app
        lease.close()
        lease.close() // closing twice counts once
        assertFalse(running.any())
        assertEquals(RemovalAtStart.ENDED, p.removal.atActivityStart())
    }

    @Test fun `without a mark nothing is touched`() = runTest {
        val p = phone(marked = false)
        assertEquals(RemovalAtStart.NONE, p.removal.atActivityStart())
        assertFalse(p.removal.startsAtConfirm(RemovalAtStart.NONE))
        assertEquals("access-r", p.tokens.getAccessToken())
        coVerify(exactly = 0) { authApi.logoutSession(any(), any(), any()) }
    }
}
