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
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
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
}
