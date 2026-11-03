package io.wolftown.kaiku.ui.approval

import androidx.fragment.app.FragmentActivity
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.Decision
import io.wolftown.kaiku.approval.KaikuSig
import io.wolftown.kaiku.approval.LoginContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.cancel
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class LoginApprovalViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val token = SigFixtures.b64u(SigFixtures.bytes(0xa0..0xbf))
    private val payload = ScanPayload.Login(SigFixtures.ORIGIN, token)
    private val api = FakeApprovalApi()

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.vm(uv: UvResult? = null, context: String = SigFixtures.V01_CONTEXT): Pair<LoginApprovalViewModel, KeyDeviceFixture> {
        val f = KeyDeviceFixture(backgroundScope, uv)
        api.nextChallenge = SigFixtures.challengeFor(SigKind.LOGIN, 1, context, "qr")
        return LoginApprovalViewModel(api, f.signer, f.repo, { SigFixtures.DEVICE_ID }, appScope = backgroundScope) to f
    }

    @Test fun `context shows client first and only verified context fields`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        val ui = v.ui.value as LoginApprovalUi.Context
        assertEquals("Kaiku desktop on Windows", ui.ctx.clientLabel)
        assertEquals("Familie Müller", ui.ctx.serverName)
        assertEquals("alice", ui.ctx.username)
        assertEquals(listOf("claim"), api.calls)
    }

    @Test fun `yes signs approve with the token as response`() = runTest {
        val (v, f) = vm()
        api.answerResult = AnswerResult("approved")
        v.start(payload)
        v.approve(activity)
        assertEquals(LoginApprovalUi.Done(R.string.approval_done), v.ui.value)
        val (decision, response, signature) = api.lastAnswer!!
        assertEquals("approve", decision)
        assertEquals(token, response)
        val m = KaikuSig.message(SigFixtures.unb64u(api.nextChallenge!!.signingPrefix), Decision.APPROVE, SigFixtures.bytes(0xa0..0xbf))
        assertTrue(SigFixtures.verifyP256(SigFixtures.b64u(f.keyStore.publicKeySpki()!!), m, signature))
    }

    @Test fun `no denies without a prompt`() = runTest {
        val (v, f) = vm()
        v.start(payload)
        v.deny()
        assertEquals(listOf("claim", "deny:${SigFixtures.challengeId(1)}"), api.calls)
        assertTrue(f.verifier.prompts.isEmpty())
        assertEquals(LoginApprovalUi.Done(R.string.approval_denied), v.ui.value)
    }

    @Test fun `i didn't start this signs report`() = runTest {
        val (v, f) = vm()
        api.answerResult = AnswerResult("reported")
        v.start(payload)
        v.report(activity)
        assertEquals("report", api.lastAnswer!!.first)
        val m = KaikuSig.message(SigFixtures.unb64u(api.nextChallenge!!.signingPrefix), Decision.REPORT, SigFixtures.bytes(0xa0..0xbf))
        assertTrue(SigFixtures.verifyP256(SigFixtures.b64u(f.keyStore.publicKeySpki()!!), m, api.lastAnswer!!.third))
        assertEquals(LoginApprovalUi.Done(R.string.approval_reported), v.ui.value)
    }

    @Test fun `password report tells to set a new password`() = runTest {
        val (v, _) = vm(context = SigFixtures.V01_CONTEXT.replace("\"variant\":\"qr\"", "\"variant\":\"password\""))
        api.answerResult = AnswerResult("reported")
        v.start(payload)
        assertEquals(io.wolftown.kaiku.approval.LoginVariant.PASSWORD, (v.ui.value as LoginApprovalUi.Context).ctx.variant)
        v.report(activity)
        assertEquals(LoginApprovalUi.Done(R.string.approval_reported_password), v.ui.value)
    }

    @Test fun `back releases the claim`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.back()
        testScheduler.runCurrent()
        assertEquals("release:${SigFixtures.challengeId(1)}", api.calls.last())
        // After a decision there is nothing to release.
        val (v2, _) = vm()
        api.calls.clear()
        v2.start(payload)
        v2.deny()
        v2.back()
        testScheduler.runCurrent()
        assertTrue(api.calls.none { it.startsWith("release") })
    }

    @Test fun `cancelled prompt goes back to the question`() = runTest {
        val (v, _) = vm(uv = UvResult.Cancelled)
        v.start(payload)
        v.approve(activity)
        assertTrue(v.ui.value is LoginApprovalUi.Context)
        assertTrue(api.calls.none { it.startsWith("answer") })
    }

    @Test fun `stale token shows the changed-code text`() = runTest {
        val (v, _) = vm()
        api.failWith = apiError(404, "LOGIN_CODE_STALE")
        v.start(payload)
        assertEquals(LoginApprovalUi.Error(R.string.approval_code_changed, rescan = true), v.ui.value)
    }

    @Test fun `other account attempt shows the other-account text`() = runTest {
        val (v, _) = vm()
        api.failWith = apiError(409, "LOGIN_ATTEMPT_OTHER_ACCOUNT")
        v.start(payload)
        assertEquals(LoginApprovalUi.Error(R.string.approval_other_account, rescan = false), v.ui.value)
        api.failWith = apiError(409, "LOGIN_ATTEMPT_CLAIMED")
        v.start(payload)
        assertEquals(LoginApprovalUi.Error(R.string.approval_already_claimed, rescan = false), v.ui.value)
    }

    @Test fun `rejected challenge releases and never prompts`() = runTest {
        val (v, f) = vm()
        api.nextChallenge = api.nextChallenge!!.copy(
            context = SigFixtures.json(SigFixtures.V01_CONTEXT.replace("Kaiku desktop on Windows", "Your own phone")),
        )
        v.start(payload)
        assertEquals(LoginApprovalUi.Error(R.string.approval_mismatch, rescan = true), v.ui.value)
        assertEquals("release:${SigFixtures.challengeId(1)}", api.calls.last())
        assertTrue(f.verifier.prompts.isEmpty())
    }

    @Test fun `expired approval asks to scan again`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        api.answerFailure = apiError(410, "APPROVAL_EXPIRED")
        v.approve(activity)
        assertEquals(LoginApprovalUi.Error(R.string.approval_expired, rescan = true), v.ui.value)
    }

    @Test fun `leaving releases the claim even after the screen's scope ended`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        // The ViewModel is being cleared: its own scope is already cancelled.
        v.viewModelScope.cancel()
        v.back()
        testScheduler.runCurrent()
        assertEquals("release:${SigFixtures.challengeId(1)}", api.calls.last())
    }
}
