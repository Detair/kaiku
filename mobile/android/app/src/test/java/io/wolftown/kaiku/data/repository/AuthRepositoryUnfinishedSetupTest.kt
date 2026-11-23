package io.wolftown.kaiku.data.repository

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.keydevice.StoredKeyDevice
import io.wolftown.kaiku.data.local.AuthSession
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.noTeardown
import io.wolftown.kaiku.ui.auth.PasswordChangeRequiredException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant

/**
 * Below the screens: while a setup of this phone is stored and not finished (or the key storage
 * cannot be read), no sign-in without the phone's key completes, whatever screen started it. The
 * session the server minted for it is ended again.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class AuthRepositoryUnfinishedSetupTest {

    private val server = "https://chat.example.com"
    private val authApi: AuthApi = mockk(relaxed = true)
    private val tokenStorage: TokenStorage = mockk(relaxed = true) {
        every { getServerUrl() } returns server
    }
    private val scope = CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher())
    private val authState = AuthState(scope)
    private val store = InMemoryKeyDeviceStore()
    private val keys = KeyDeviceRepository(store, SoftwareApprovalKeyStore(), authState, scope)
    private val repository = AuthRepository(authApi, tokenStorage, authState, mockk<PushRegistrar>(relaxed = true), noTeardown(authState))
        .also { it.keyDevicePolicy = keys }

    private fun response(access: String) = AuthResponse(accessToken = access, refreshToken = "r-$access", expiresIn = 900, tokenType = "Bearer")

    private fun stored(phase: KeyDevicePhase?) {
        store.stored = phase?.let {
            StoredKeyDevice(
                LocalKeyDevice(null, "first-account", server, UvClass.PER_USE, "e1", null),
                it,
                readyAt = if (it == KeyDevicePhase.PENDING) Instant.parse("2026-11-21T10:00:00Z") else null,
            )
        }
        keys.reload()
    }

    @Test fun `no password, passkey or single sign-on completes while an unfinished setup is stored`() = runTest {
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns User(id = "other-account", username = "bob", displayName = "Bob")
        for (phase in listOf(KeyDevicePhase.CLAIMING, KeyDevicePhase.CLAIMED, KeyDevicePhase.PENDING)) {
            stored(phase)
            // A password or single sign-on attempt (both end in a collected attempt) ...
            assertTrue(phase.name, repository.completeAttempt(server, "pw") { response("a-$phase") }.isFailure)
            // ... and a passkey.
            assertTrue(phase.name, repository.installNativeSignIn(response("p-$phase"), server).isFailure)
            assertEquals(phase.name, AuthSession.LoggedOut, authState.session.value)
            coVerify { authApi.logoutSession(server, "a-$phase", any()) }
            coVerify { authApi.logoutSession(server, "p-$phase", any()) }
        }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test fun `nothing completes while the key storage cannot be read`() = runTest {
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns User(id = "u1", username = "alice", displayName = "Alice")
        store.unreadable = true
        keys.reload()
        assertTrue(repository.completeAttempt(server, "pw") { response("a") }.isFailure)
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    @Test fun `once the setup has ended a sign-in completes as before`() = runTest {
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns User(id = "u1", username = "alice", displayName = "Alice")
        stored(KeyDevicePhase.CLAIMING)
        stored(null)
        assertTrue(repository.completeAttempt(server, "pw") { response("a") }.isSuccess)
        assertTrue(authState.session.value is AuthSession.LoggedIn)
    }

    /**
     * The key of this phone waits for its device sign-in on its own server (also while that word
     * is only provisional): an ordinary sign-in on another server is not installed, and the session
     * that server minted for it is ended. On the key's own server it completes as before.
     */
    @Test fun `no ordinary sign-in on another server is installed over a key that waits for its device sign-in`() = runTest {
        val other = "https://other.example.org"
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns User(id = "other-account", username = "bob", displayName = "Bob")
        for (provisional in listOf(null, KeyDevicePhase.CLAIMED)) {
            store.stored = StoredKeyDevice(
                LocalKeyDevice(null, "first-account", server, UvClass.PER_USE, "e1", null),
                KeyDevicePhase.ACTIVE,
                provisionalFrom = provisional,
            )
            keys.reload()
            every { tokenStorage.getServerUrl() } returns other
            // Refused with the reason, so that the sign-in screen can say why.
            assertTrue(repository.completeAttempt(other, "pw") { response("a-$provisional") }.exceptionOrNull() is KeyOnOtherServerException)
            assertTrue(repository.installNativeSignIn(response("p-$provisional"), other).exceptionOrNull() is KeyOnOtherServerException)
            assertEquals(AuthSession.LoggedOut, authState.session.value)
            coVerify { authApi.logoutSession(other, "a-$provisional", any()) }
            coVerify { authApi.logoutSession(other, "p-$provisional", any()) }
            // A sign-in that first needs a new password: refused before anything is changed there, and ended.
            val change = response("c-$provisional").copy(passwordChangeRequired = true)
            val refused = repository.completeAttempt(other, "pw") { change }
            assertTrue(refused.exceptionOrNull() is KeyOnOtherServerException)
            assertFalse(refused.exceptionOrNull() is PasswordChangeRequiredException)
            assertFalse(repository.hasPendingPasswordChange())
            coVerify { authApi.logoutSession(other, "c-$provisional", any()) }
        }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        coVerify(exactly = 0) { authApi.changePassword(any(), any(), any(), any()) }
        // On the key's own server another account may sign in as before.
        every { tokenStorage.getServerUrl() } returns server
        assertTrue(repository.completeAttempt(server, "pw") { response("own") }.isSuccess)
    }

    /** A key whose server was stored in another spelling (case, default port, trailing slash) is still on the same server. */
    @Test fun `an ordinary sign-in on the key's own server is installed whatever spelling the key stored`() = runTest {
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns User(id = "other-account", username = "bob", displayName = "Bob")
        store.stored = StoredKeyDevice(
            LocalKeyDevice(null, "first-account", "HTTPS://Chat.Example.com:443/", UvClass.PER_USE, "e1", null),
            KeyDevicePhase.ACTIVE,
        )
        keys.reload()
        assertTrue(repository.completeAttempt(server, "pw") { response("own") }.isSuccess)
        assertTrue(authState.session.value is AuthSession.LoggedIn)
    }

    /**
     * A sign-in on another server that needs a new password was waiting for it when this phone's
     * key became active for its own server: choosing the new password changes nothing on the other
     * server, and its session is ended.
     */
    @Test fun `a waiting password change is not made once the key waits on another server`() = runTest {
        val other = "https://other.example.org"
        every { tokenStorage.getServerUrl() } returns other
        val change = response("c").copy(passwordChangeRequired = true)
        assertTrue(repository.completeAttempt(other, "pw") { change }.exceptionOrNull() is PasswordChangeRequiredException)
        store.stored = StoredKeyDevice(LocalKeyDevice(null, "first-account", server, UvClass.PER_USE, "e1", null), KeyDevicePhase.ACTIVE)
        keys.reload()
        assertTrue(repository.completePasswordChange("new-password").exceptionOrNull() is KeyOnOtherServerException)
        coVerify(exactly = 0) { authApi.changePassword(any(), any(), any(), any()) }
        assertFalse(repository.hasPendingPasswordChange())
        coVerify { authApi.logoutSession(other, "c", any()) }
    }

    /**
     * A setup of this phone for server A is still running (claim sent, code shown, or waiting for
     * its ready time). A sign-in on server B that first needs a new password is refused before
     * anything is kept or changed there: no pending change, no new password sent, its session ended.
     * A change that was already waiting when such a setup started is not made either.
     */
    @Test fun `no password change is kept or made while a setup for another server runs`() = runTest {
        val other = "https://other.example.org"
        every { tokenStorage.getServerUrl() } returns other
        for (phase in listOf(KeyDevicePhase.CLAIMING, KeyDevicePhase.CLAIMED, KeyDevicePhase.PENDING)) {
            stored(phase)
            val change = response("c-$phase").copy(passwordChangeRequired = true)
            val refused = repository.completeAttempt(other, "pw") { change }
            assertTrue(phase.name, refused.isFailure)
            assertFalse(phase.name, refused.exceptionOrNull() is PasswordChangeRequiredException)
            assertFalse(phase.name, repository.hasPendingPasswordChange())
            coVerify { authApi.logoutSession(other, "c-$phase", any()) }

            // Waiting already when the setup started.
            stored(null)
            val waiting = response("w-$phase").copy(passwordChangeRequired = true)
            assertTrue(repository.completeAttempt(other, "pw") { waiting }.exceptionOrNull() is PasswordChangeRequiredException)
            stored(phase)
            assertTrue(phase.name, repository.completePasswordChange("new-password").isFailure)
            assertFalse(phase.name, repository.hasPendingPasswordChange())
            coVerify { authApi.logoutSession(other, "w-$phase", any()) }
        }
        coVerify(exactly = 0) { authApi.changePassword(any(), any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
    }

    /**
     * The key became active for server A while the new password was being set on server B: the
     * session on B is not installed and is ended, and the person is told why.
     */
    @Test fun `a key that became active while the password was changed keeps the session out`() = runTest {
        val other = "https://other.example.org"
        every { tokenStorage.getServerUrl() } returns other
        coEvery { authApi.authenticatedGetMe(any(), any()) } returns User(id = "other-account", username = "bob", displayName = "Bob")
        val change = response("c").copy(passwordChangeRequired = true)
        assertTrue(repository.completeAttempt(other, "pw") { change }.exceptionOrNull() is PasswordChangeRequiredException)
        coEvery { authApi.changePassword(any(), any(), any(), any()) } coAnswers {
            store.stored = StoredKeyDevice(LocalKeyDevice(null, "first-account", server, UvClass.PER_USE, "e1", null), KeyDevicePhase.ACTIVE)
            keys.reload()
            Unit
        }
        val result = repository.completePasswordChange("new-password")
        assertTrue(result.exceptionOrNull() is KeyOnOtherServerException)
        assertEquals(AuthSession.LoggedOut, authState.session.value)
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertFalse(repository.hasPendingPasswordChange())
        coVerify { authApi.logoutSession(other, "c", any()) }
    }
}
