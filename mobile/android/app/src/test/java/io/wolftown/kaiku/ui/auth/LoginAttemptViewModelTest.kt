package io.wolftown.kaiku.ui.auth

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.model.AttemptState
import io.wolftown.kaiku.domain.model.AuthResponse
import io.wolftown.kaiku.domain.model.LoginStep
import io.wolftown.kaiku.domain.model.LoginStepStatus
import io.wolftown.kaiku.domain.model.User
import io.wolftown.kaiku.testutil.FakeLoginAttemptApi
import io.wolftown.kaiku.testutil.FakeLoginAttemptApi.Companion.attempt
import io.wolftown.kaiku.testutil.apiError
import io.wolftown.kaiku.testutil.s256
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
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
import java.io.File
import java.io.IOException

@OptIn(ExperimentalCoroutinesApi::class)
class LoginAttemptViewModelTest {

    private val server = "https://chat.example.com"
    private val api = FakeLoginAttemptApi()
    private val tokenStorage: TokenStorage = mockk(relaxed = true)
    private val repo: AuthRepository = mockk(relaxed = true)
    private val user = User("u-1", "anna", "Anna")
    /** Every session [AuthRepository.completeAttempt] was asked to install, with the typed password it got. */
    private val installs = mutableListOf<Pair<AuthResponse, String?>>()

    @Before
    fun setUp() {
        Dispatchers.setMain(StandardTestDispatcher())
        every { tokenStorage.getServerUrl() } returns server
        coEvery { repo.completeAttempt(any(), any(), any()) } coAnswers {
            val collect = thirdArg<suspend () -> AuthResponse>()
            installs += collect() to secondArg<String?>()
            Result.success(user)
        }
    }

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.vm() = LoginAttemptViewModel(api, repo, tokenStorage, mockk(relaxed = true), appScope = backgroundScope)

    private fun confirmationStep(totp: Boolean = true, qr: String = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=P1") =
        LoginStep(LoginStepStatus.CONFIRMATION_REQUIRED, attempt("a-pw", "password", qr = qr, totpAvailable = totp))

    @Test fun `password login shows the confirmation qr`() = runTest {
        api.loginStep = confirmationStep()
        val v = vm()
        v.signInWithPassword("anna", "pw")
        runCurrent()

        val ui = v.state.value.screen as LoginAttemptUi.ShowQr
        assertEquals("kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=P1", ui.payload)
        assertEquals(AttemptVia.PASSWORD, ui.via)
        assertTrue(ui.totpAvailable)
        assertTrue(ui.minutesLeft in 9..10)
        // The attempt is bound to this screen's verifier: S256(verifier) went out at the login.
        assertEquals("poll:a-pw", api.calls.last())
        assertEquals(s256(api.verifiers.single()), api.challenges.single())
        assertEquals(43, api.challenges.single().length)
        assertNull(v.state.value.formError)
    }

    @Test fun `totp_required shows the code field`() = runTest {
        api.loginStep = LoginStep(LoginStepStatus.TOTP_REQUIRED, attempt("a-pw", "password"))
        val v = vm()
        v.signInWithPassword("anna", "pw")
        advanceUntilIdle()

        assertEquals(LoginAttemptUi.Totp(error = null), v.state.value.screen)
        assertEquals(listOf("login:anna"), api.calls)
    }

    @Test fun `phone not at hand keeps the attempt`() = runTest {
        api.loginStep = confirmationStep(totp = true)
        val v = vm()
        v.signInWithPassword("anna", "pw")
        runCurrent()
        v.phoneNotAtHand()
        runCurrent()
        assertEquals(LoginAttemptUi.Totp(error = null), v.state.value.screen)

        v.submitTotp("123 456")
        advanceUntilIdle()

        // Same attempt, same verifier, no cancel and no second sign-in.
        assertEquals(listOf("login:anna", "poll:a-pw", "totp:a-pw:123456"), api.calls)
        assertEquals(1, api.verifiers.toSet().size)
        assertTrue(v.state.value.signedIn)
        // The typed password stays with the attempt (C4: a forced change needs it).
        assertEquals("pw", installs.single().second)
    }

    @Test fun `phone not at hand is only offered with totp`() = runTest {
        api.loginStep = confirmationStep(totp = false)
        val v = vm()
        v.signInWithPassword("anna", "pw")
        runCurrent()
        v.phoneNotAtHand()
        runCurrent()
        assertTrue(v.state.value.screen is LoginAttemptUi.ShowQr)
    }

    @Test fun `a wrong totp code stays on the code step`() = runTest {
        api.loginStep = LoginStep(LoginStepStatus.TOTP_REQUIRED, attempt("a-pw", "password"))
        coEvery { repo.completeAttempt(any(), any(), any()) } returns Result.failure(apiError(401, "INVALID_MFA"))
        val v = vm()
        v.signInWithPassword("anna", "pw")
        advanceUntilIdle()
        v.submitTotp("000000")
        advanceUntilIdle()
        assertEquals(LoginAttemptUi.Totp(error = R.string.auth_login_mfa_invalid), v.state.value.screen)

        coEvery { repo.completeAttempt(any(), any(), any()) } returns Result.failure(apiError(403, "TOTP_UNAVAILABLE"))
        v.submitTotp("111111")
        advanceUntilIdle()
        assertEquals(LoginAttemptUi.Totp(error = R.string.auth_totp_unavailable), v.state.value.screen)
    }

    @Test fun `qr from the other phone rotates and collects once`() = runTest {
        api.created = attempt("a-qr", "qr", qr = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=T1")
        val approve = CompletableDeferred<Unit>()
        api.polls += { attempt("a-qr", "qr", qr = "kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=T2") }
        api.polls += { approve.await(); attempt("a-qr", "qr", state = AttemptState.APPROVED) }
        val v = vm()
        v.startQr()
        runCurrent()

        val shown = v.state.value.screen as LoginAttemptUi.ShowQr
        assertEquals(AttemptVia.OTHER_PHONE, shown.via)
        // A poll in `open` minted a new token: same attempt, new image.
        assertEquals("kaiku://login/v1?o=https%3A%2F%2Fchat.example.com&t=T2", shown.payload)
        assertEquals(listOf("createQr", "poll:a-qr", "poll:a-qr"), api.calls)

        approve.complete(Unit)
        advanceUntilIdle()

        assertEquals(1, api.calls.count { it == "collect:a-qr" })
        assertEquals(1, installs.size)
        assertNull(installs.single().second)
        assertTrue(v.state.value.signedIn)
        assertEquals(s256(api.verifiers.first()), api.challenges.single())
    }

    @Test fun `network error while polling retries and keeps the attempt`() = runTest {
        api.polls += { throw IOException("offline") }
        api.polls += { throw IOException("offline") }
        val v = vm()
        v.startQr()
        runCurrent()
        assertEquals(1, api.calls.count { it == "poll:a-qr" })

        // Retries after 1 s, then 2 s (then every 5 s while the attempt is live).
        advanceTimeBy(1_001)
        runCurrent()
        assertEquals(2, api.calls.count { it == "poll:a-qr" })
        advanceTimeBy(2_001)
        runCurrent()
        assertEquals(3, api.calls.count { it == "poll:a-qr" })
        assertTrue(v.state.value.screen is LoginAttemptUi.ShowQr)
        assertEquals(1, api.calls.count { it == "createQr" })
    }

    @Test fun `ended attempts say why`() = runTest {
        api.polls += { attempt("a-qr", "qr", state = AttemptState.DENIED) }
        val v = vm()
        v.startQr()
        advanceUntilIdle()
        assertEquals(LoginAttemptUi.Ended(R.string.auth_phone_ended_denied, AttemptVia.OTHER_PHONE), v.state.value.screen)

        api.polls += { throw apiError(410, "LOGIN_ATTEMPT_ENDED", ApiErrorFields(state = "reported")) }
        v.tryAgain()
        advanceUntilIdle()
        assertEquals(LoginAttemptUi.Ended(R.string.auth_phone_ended_reported, AttemptVia.OTHER_PHONE), v.state.value.screen)
        assertEquals(2, api.calls.count { it == "createQr" })
    }

    @Test fun `leaving cancels the attempt`() = runTest {
        val v = vm()
        v.startQr()
        runCurrent()
        val verifier = api.verifiers.single()

        v.leave()
        runCurrent() // the cancel runs in the app scope, so it outlives the screen

        assertNull(v.state.value.screen)
        assertTrue("cancel:a-qr" in api.calls)
        assertEquals(verifier, api.verifiers.last())
        // The long-poll stopped with it: nothing else is sent for this attempt.
        val after = api.calls.size
        advanceTimeBy(60_000)
        assertEquals(after, api.calls.size)
        coVerify(exactly = 0) { repo.completeAttempt(any(), any(), any()) }
    }

    @Test fun `neutral answer text for INVALID_CREDENTIALS`() = runTest {
        api.loginFailure = apiError(401, "INVALID_CREDENTIALS")
        val v = vm()
        v.signInWithPassword("anna", "wrong")
        advanceUntilIdle()

        assertNull(v.state.value.screen)
        assertEquals(R.string.auth_login_invalid_credentials, v.state.value.formError)
        // F3's one neutral answer, word for word (spec F3 step 1).
        assertEquals(
            "Wrong username or password, or password sign-in is not available for this account. " +
                "Use a passkey or 'Sign in with phone', or ask an admin.",
            appString("auth_login_invalid_credentials"),
        )
    }

    @Test fun `too many attempts says how long to wait`() = runTest {
        api.loginFailure = apiError(429, "TOO_MANY_ATTEMPTS", ApiErrorFields(retryAfterSeconds = 300))
        val v = vm()
        v.signInWithPassword("anna", "pw")
        advanceUntilIdle()
        assertEquals(5, v.state.value.retryMinutes)
        assertNull(v.state.value.formError)

        api.loginFailure = apiError(400, "CLIENT_OUTDATED")
        v.signInWithPassword("anna", "pw")
        advanceUntilIdle()
        assertEquals(R.string.auth_login_client_outdated, v.state.value.formError)
        assertNull(v.state.value.retryMinutes)
    }

    @Test fun `a verifier is drawn per attempt and never shown`() = runTest {
        val v = vm()
        v.startQr()
        runCurrent()
        v.leave()
        advanceUntilIdle()
        v.startQr()
        runCurrent()
        assertEquals(2, api.challenges.toSet().size)
        assertFalse(v.state.value.toString().contains(api.verifiers.first()))
        assertFalse(v.state.value.screen.toString().contains("T1"))
    }

    /** The default (English) text of a string resource, read from `res/values/strings.xml`. */
    private fun appString(name: String): String {
        val xml = File("src/main/res/values/strings.xml").readText()
        val raw = Regex("""<string name="$name">(.*?)</string>""").find(xml)?.groupValues?.get(1)
            ?: error("no string $name")
        return raw.replace("\\'", "'").replace("\\\"", "\"")
    }
}
