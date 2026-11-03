package io.wolftown.kaiku.ui.auth

import android.content.Context
import android.net.Uri
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.mockk.spyk
import io.mockk.verify
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.LoginStep
import io.wolftown.kaiku.domain.model.LoginStepStatus
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.FakeLoginAttemptApi
import io.wolftown.kaiku.testutil.FakeLoginAttemptApi.Companion.attempt
import io.wolftown.kaiku.testutil.s256
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class OidcHandlerTest {

    private val server = "https://chat.example.com"
    private val tokenStorage: TokenStorage = mockk(relaxed = true)
    private val repo: AuthRepository = mockk(relaxed = true)
    private val api = FakeLoginAttemptApi()
    private val context: Context = mockk(relaxed = true)

    @Before
    fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        every { tokenStorage.getServerUrl() } returns server
        coEvery { repo.completeAttempt(any(), any(), any()) } returns Result.success(User("u-1", "anna", "Anna"))
    }

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun callback(params: Map<String, String>): Uri = mockk {
        every { scheme } returns "kaiku"
        every { host } returns "auth"
        every { path } returns "/callback"
        every { queryParameterNames } returns params.keys
        every { getQueryParameter(any()) } answers { params[firstArg<String>()] }
    }

    private fun TestScope.vm(handler: OidcHandler) =
        LoginAttemptViewModel(api, repo, tokenStorage, handler, appScope = backgroundScope)

    @Test fun `oidc callback with tokens installs nothing`() = runTest {
        val handler = spyk(OidcHandler(tokenStorage))
        every { handler.launchOidcLogin(any(), any(), any()) } returns Unit
        val tokens = callback(mapOf("access_token" to "stolen-access", "refresh_token" to "stolen-refresh", "expires_in" to "900"))
        assertNull(handler.handleCallback(tokens))
        // A code next to tokens is refused as a whole.
        assertNull(handler.handleCallback(callback(mapOf("code" to "c", "access_token" to "stolen-access"))))
        assertNull(handler.handleCallback(callback(mapOf("error" to "access_denied"))))

        val v = vm(handler)
        v.startOidc(context, "google")
        v.onOidcCallback(tokens)
        advanceUntilIdle()

        assertTrue(api.calls.none { it.startsWith("exchangeOidc") })
        coVerify(exactly = 0) { repo.completeAttempt(any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(R.string.auth_login_sso_failed, v.state.value.formError)
        assertNull(v.state.value.screen)
    }

    @Test fun `oidc code is exchanged with the attempt verifier`() = runTest {
        val handler = spyk(OidcHandler(tokenStorage))
        var challenge: String? = null
        every { handler.launchOidcLogin(any(), any(), any()) } answers { challenge = thirdArg() }
        api.exchangeStep = LoginStep(
            LoginStepStatus.CONFIRMATION_REQUIRED,
            attempt("a-oidc", "oidc", qr = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=O1"),
        )
        val v = vm(handler)
        v.startOidc(context, "google")
        v.onOidcCallback(callback(mapOf("code" to "one-time")))
        runCurrent()

        assertEquals("exchangeOidc:one-time", api.calls.first())
        assertEquals(challenge, s256(api.verifiers.first()))
        val ui = v.state.value.screen as LoginAttemptUi.ShowQr
        assertEquals(AttemptVia.OIDC, ui.via)
        // The attempt's polls carry the same verifier the start's challenge was made from.
        assertEquals(1, api.verifiers.toSet().size)
    }

    @Test fun `a callback without a waiting sign-in exchanges nothing`() = runTest {
        val v = vm(OidcHandler(tokenStorage))
        v.onOidcCallback(callback(mapOf("code" to "one-time")))
        advanceUntilIdle()
        assertTrue(api.calls.isEmpty())
        assertEquals(R.string.auth_login_sso_failed, v.state.value.formError)
    }

    @Test fun `the start sends only the attempt's client challenge`() {
        val url = OidcHandler(tokenStorage).authorizeUrl(server, "google", "CHALLENGE")
        assertEquals(
            "$server/auth/oidc/authorize/google?redirect_uri=kaiku%3A%2F%2Fauth%2Fcallback&client_challenge=CHALLENGE",
            url,
        )
        assertFalse(url.contains("state="))
        assertFalse(url.contains("code_challenge"))
        // Nothing about the flow is written to storage (the verifier lives in memory only).
        verify(exactly = 0) { tokenStorage.saveOidcPkceState(any(), any()) }
    }
}
