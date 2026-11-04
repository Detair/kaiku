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
        val handler = spyk(OidcHandler())
        every { handler.openProvider(any(), any()) } returns Unit
        val tokens = callback(mapOf("access_token" to "stolen-access", "refresh_token" to "stolen-refresh", "expires_in" to "900"))
        assertNull(handler.handleCallback(tokens))
        // A code next to tokens is refused as a whole.
        assertNull(handler.handleCallback(callback(mapOf("code" to "c", "access_token" to "stolen-access"))))
        // An error callback is not a code (it is reported, see below).
        assertEquals(OidcCallback.Error(null), handler.handleCallback(callback(mapOf("error" to "access_denied"))))

        val v = vm(handler)
        v.startOidc(context, "google")
        runCurrent()
        v.onOidcCallback(tokens)
        advanceUntilIdle()

        assertEquals(listOf("oidcStart:google"), api.calls)
        coVerify(exactly = 0) { repo.completeAttempt(any(), any(), any()) }
        verify(exactly = 0) { tokenStorage.saveTokens(any(), any(), any(), any()) }
        assertEquals(R.string.auth_login_sso_failed, v.state.value.formError)
        assertNull(v.state.value.screen)
    }

    @Test fun `oidc code is exchanged with the attempt verifier`() = runTest {
        val handler = spyk(OidcHandler())
        every { handler.openProvider(any(), any()) } returns Unit
        api.exchangeStep = LoginStep(
            LoginStepStatus.CONFIRMATION_REQUIRED,
            attempt("a-oidc", "oidc", qr = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=O1"),
        )
        val v = vm(handler)
        v.startOidc(context, "google")
        runCurrent()
        // The start was fetched (with the challenge), then its provider page opened.
        verify { handler.openProvider(context, "https://idp.example.com/authorize?state=s1") }
        assertEquals(false, v.state.value.busy)
        v.onOidcCallback(callback(mapOf("code" to "one-time")))
        runCurrent()

        assertEquals(listOf("oidcStart:google", "exchangeOidc:one-time"), api.calls.take(2))
        assertEquals(api.challenges.single(), s256(api.verifiers.first()))
        val ui = v.state.value.screen as LoginAttemptUi.ShowQr
        assertEquals(AttemptVia.OIDC, ui.via)
        // The attempt's polls carry the same verifier the start's challenge was made from.
        assertEquals(1, api.verifiers.toSet().size)
    }

    @Test fun `a callback without a waiting sign-in exchanges nothing`() = runTest {
        val v = vm(OidcHandler())
        v.onOidcCallback(callback(mapOf("code" to "one-time")))
        advanceUntilIdle()
        assertTrue(api.calls.isEmpty())
        assertEquals(R.string.auth_login_sso_failed, v.state.value.formError)
    }

    /**
     * Fable v1 final check, B1: the server hands a failed sign-in back as
     * `kaiku://auth/callback?error=CODE`; the form says why, through
     * [AuthErrorMapper.oidc], and nothing is exchanged.
     */
    @Test fun `an error callback shows the server's reason`() = runTest {
        val handler = spyk(OidcHandler())
        every { handler.openProvider(any(), any()) } returns Unit
        assertEquals(
            OidcCallback.Error("INVALID_CREDENTIALS"),
            handler.handleCallback(callback(mapOf("error" to "INVALID_CREDENTIALS"))),
        )
        for ((code, text) in listOf(
            "INVALID_CREDENTIALS" to R.string.auth_login_invalid_credentials,
            "OIDC_CLIENT_OUTDATED" to R.string.auth_login_client_outdated,
            "SERVICE_UNAVAILABLE" to R.string.errors_server,
            "OIDC_PROVIDER_ERROR" to R.string.auth_login_sso_failed,
        )) {
            val v = vm(handler)
            v.startOidc(context, "google")
            runCurrent()
            v.onOidcCallback(callback(mapOf("error" to code)))
            advanceUntilIdle()
            assertEquals(code, text, v.state.value.formError)
            assertFalse(v.state.value.busy)
        }
        assertTrue(api.calls.none { it.startsWith("exchangeOidc") })
    }

    /**
     * Codex v1 integration check, major 2: a refused start (an older server, or
     * no `{url}`) shows its error and opens no Custom Tab; no callback is awaited.
     */
    @Test fun `a refused oidc start opens nothing and says why`() = runTest {
        val handler = spyk(OidcHandler())
        every { handler.openProvider(any(), any()) } returns Unit
        api.oidcStartFailure = io.wolftown.kaiku.data.api.ApiException(
            io.ktor.http.HttpStatusCode.BadRequest, "outdated", code = "OIDC_CLIENT_OUTDATED",
        )
        val v = vm(handler)
        v.startOidc(context, "google")
        advanceUntilIdle()
        verify(exactly = 0) { handler.openProvider(any(), any()) }
        assertEquals(R.string.auth_login_client_outdated, v.state.value.formError)
        assertFalse(v.state.value.busy)
        v.onOidcCallback(callback(mapOf("code" to "one-time")))
        advanceUntilIdle()
        assertTrue(api.calls.none { it.startsWith("exchangeOidc") })
    }
}
