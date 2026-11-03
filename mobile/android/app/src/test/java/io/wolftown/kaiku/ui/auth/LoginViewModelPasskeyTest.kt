package io.wolftown.kaiku.ui.auth

import android.app.Activity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.PasskeyApi
import io.wolftown.kaiku.data.api.PasskeyApiException
import io.wolftown.kaiku.data.api.ProbeResult
import io.wolftown.kaiku.data.api.ServerProbe
import io.wolftown.kaiku.data.auth.PasskeyAuth
import io.wolftown.kaiku.data.auth.PasskeyCredentials
import io.wolftown.kaiku.data.auth.PasskeyOutcome
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.CeremonyStart
import io.wolftown.kaiku.domain.model.User
import io.ktor.http.HttpStatusCode
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class LoginViewModelPasskeyTest {
    private val dispatcher = StandardTestDispatcher()
    private val api: PasskeyApi = mockk()
    private val credentials: PasskeyCredentials = mockk(relaxed = true)
    private val repo: AuthRepository = mockk(relaxed = true)
    private val tokenStorage: TokenStorage = mockk(relaxed = true)
    private val activity: Activity = mockk(relaxed = true)
    private lateinit var vm: LoginViewModel

    private val start = CeremonyStart(
        "c-1",
        Json.parseToJsonElement("""{"publicKey":{"challenge":"AQ","rpId":"chat.example.com"}}""").jsonObject,
    )
    private val assertion = """{"id":"Y3JlZA","type":"public-key","response":{}}"""

    @Before
    fun setUp() {
        Dispatchers.setMain(dispatcher)
        every { tokenStorage.getServerUrl() } returns "https://chat.example.com"
        every { repo.canStartSignIn() } returns true
        val probe: ServerProbe = mockk()
        coEvery { probe.probe(any()) } returns ProbeResult.Ok(registrationOpen = false)
        coEvery { api.loginStart() } returns start
        coEvery { credentials.get(any(), any()) } returns PasskeyOutcome.Success(assertion)
        vm = LoginViewModel(repo, mockk(relaxed = true), tokenStorage, probe, PasskeyAuth(api, credentials, repo, tokenStorage))
    }

    @After
    fun tearDown() = Dispatchers.resetMain()

    @Test
    fun `success installs the session`() = runTest(dispatcher) {
        coEvery { api.loginFinish(any(), any(), any()) } returns mockk(relaxed = true)
        coEvery { repo.installNativeSignIn(any(), any()) } returns Result.success(mockk<User>(relaxed = true))
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        assertTrue(vm.uiState.value.loginSuccess)
        coVerify { credentials.get(activity, """{"challenge":"AQ","rpId":"chat.example.com"}""") }
    }

    @Test
    fun `cancelled shows no error`() = runTest(dispatcher) {
        coEvery { credentials.get(any(), any()) } returns PasskeyOutcome.Cancelled
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        assertNull(vm.uiState.value.passkeyErrorRes)
        assertFalse(vm.uiState.value.isLoading)
    }

    @Test
    fun `no credential and not associated have their own sentences`() = runTest(dispatcher) {
        coEvery { credentials.get(any(), any()) } returns PasskeyOutcome.NoCredential
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        assertEquals(R.string.passkeys_android_none_on_device, vm.uiState.value.passkeyErrorRes)
        coEvery { credentials.get(any(), any()) } returns PasskeyOutcome.NotAssociated
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        assertEquals(R.string.passkeys_android_not_set_up_for_app, vm.uiState.value.passkeyErrorRes)
    }

    @Test
    fun `server PASSKEY_UNKNOWN signals and says removed`() = runTest(dispatcher) {
        coEvery { api.loginFinish(any(), any(), any()) } throws PasskeyApiException(HttpStatusCode.BadRequest, "PASSKEY_UNKNOWN")
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        assertEquals(R.string.passkeys_error_removed, vm.uiState.value.passkeyErrorRes)
        coVerify { credentials.signalRemoved("chat.example.com", null, "Y3JlZA", null) }
    }

    @Test
    fun `server PASSKEY_PENDING sets passkeyReadyAt and signals nothing`() = runTest(dispatcher) {
        coEvery { api.loginFinish(any(), any(), any()) } throws
            PasskeyApiException(HttpStatusCode.Conflict, "PASSKEY_PENDING", "2026-11-05T10:00:00Z")
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        assertNotNull(vm.uiState.value.passkeyReadyAt)
        assertEquals(R.string.passkeys_error_ready_at, vm.uiState.value.passkeyErrorRes)
        coVerify(exactly = 0) { credentials.signalRemoved(any(), any(), any(), any()) }
    }

    @Test
    fun `a passkey tap while a password sign-in runs starts nothing`() = runTest(dispatcher) {
        // L2b review (Argon p5 #6), deferred to LP: the button has its own
        // `enabled`, and the ViewModel refuses a start while another sign-in runs.
        val login = CompletableDeferred<Result<User>>()
        coEvery { repo.login(any(), any(), any()) } coAnswers { login.await() }
        vm.onLoginClicked()
        advanceUntilIdle()
        assertTrue(vm.uiState.value.isLoading)
        assertFalse("the passkey label stays idle", vm.uiState.value.passkeyLoading)
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        coVerify(exactly = 0) { api.loginStart() }
        // The password sign-in was not cancelled and still completes.
        login.complete(Result.success(mockk(relaxed = true)))
        advanceUntilIdle()
        assertTrue(vm.uiState.value.loginSuccess)
    }

    @Test
    fun `a second passkey tap while one runs starts no second ceremony`() = runTest(dispatcher) {
        val outcome = CompletableDeferred<PasskeyOutcome>()
        coEvery { credentials.get(any(), any()) } coAnswers { outcome.await() }
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        assertTrue(vm.uiState.value.passkeyLoading)
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        coVerify(exactly = 1) { api.loginStart() }
        outcome.complete(PasskeyOutcome.Cancelled)
        advanceUntilIdle()
        assertFalse(vm.uiState.value.isLoading)
        assertFalse(vm.uiState.value.passkeyLoading)
    }

    @Test
    fun `a password sign-in that replaces a passkey one clears the passkey label`() = runTest(dispatcher) {
        coEvery { credentials.get(any(), any()) } coAnswers { CompletableDeferred<PasskeyOutcome>().await() }
        coEvery { repo.login(any(), any(), any()) } returns Result.success(mockk(relaxed = true))
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        vm.onLoginClicked()
        advanceUntilIdle()
        assertTrue(vm.uiState.value.loginSuccess)
        assertFalse(vm.uiState.value.passkeyLoading)
    }

    @Test
    fun `already signed in starts no ceremony`() = runTest(dispatcher) {
        // Reviews Argon minor 3 / Argon-a m1: nothing is minted on the server.
        every { repo.canStartSignIn() } returns false
        vm.onPasskeySignIn(activity)
        advanceUntilIdle()
        coVerify(exactly = 0) { api.loginStart() }
        coVerify(exactly = 0) { api.loginFinish(any(), any(), any()) }
    }
}
