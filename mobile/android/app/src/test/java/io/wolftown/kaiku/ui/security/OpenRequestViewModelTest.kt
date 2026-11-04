package io.wolftown.kaiku.ui.security

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.auth.AccountStepUp
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.CancelVia
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.testutil.FakeSecurityApi
import io.wolftown.kaiku.testutil.RecordingPasskeyStepUp
import io.wolftown.kaiku.testutil.apiError
import io.wolftown.kaiku.testutil.nonKeyPhoneStepUp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
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
class OpenRequestViewModelTest {
    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val api = FakeSecurityApi()
    private val request = AccountRequestSummary("rq1", AccountRef("u1", "alice"), kind = "self", purpose = "recovery",
        state = "waiting", cancel = CancelVia("step_up"))

    private fun withRequest(r: AccountRequestSummary? = request) {
        api.me = api.me.copy(openRequest = r, prompts = if (r == null) emptyList() else listOf("open_request"))
    }

    @Test fun `open request prompt shows at app open and cancel steps up`() = runTest {
        withRequest()
        api.cancelResult = request.copy(state = "cancelled")
        val stepUp: AccountStepUp = mockk()
        coEvery { stepUp.stepUp(activity, StepUpAction.REQUEST_CANCEL, "rq1") } returns
            StepUpOutcome.Granted(StepUpGrant("request.cancel", "rq1", "device", "t"))
        val v = OpenRequestViewModel(api, stepUp)

        v.refresh("u1")
        assertEquals(request, v.pending.value)
        v.cancel(activity)
        coVerify { stepUp.stepUp(activity, StepUpAction.REQUEST_CANCEL, "rq1") }
        assertTrue("cancelRequest:rq1" in api.calls)
        assertNull(v.pending.value)
        assertFalse(v.working.value)
    }

    @Test fun `a completed or missing request shows nothing`() = runTest {
        val v = OpenRequestViewModel(api, mockk())
        v.refresh("u1")
        assertNull(v.pending.value)
        withRequest(request.copy(state = "completed"))
        v.refresh("u1")
        assertNull(v.pending.value)
    }

    @Test fun `a request started while in the background shows at the next foreground`() = runTest {
        val v = OpenRequestViewModel(api, mockk())
        v.refresh("u1") // app open: nothing yet
        assertNull(v.pending.value)
        withRequest() // someone starts a recovery while the app is in the background
        v.refresh("u1") // foreground again, e.g. straight into a chat
        assertEquals(request, v.pending.value)
    }

    @Test fun `another account or a sign-out starts from nothing`() = runTest {
        withRequest()
        val v = OpenRequestViewModel(api, mockk())
        v.refresh("u1")
        v.dismiss()
        assertNull(v.pending.value)

        // u2 signs in and its read fails: nothing of u1 is shown or remembered.
        withRequest(request.copy(subject = AccountRef("u2", "bob")))
        api.meFailure = java.io.IOException("offline")
        v.refresh("u2")
        assertNull(v.pending.value)
        api.meFailure = null
        v.refresh("u2")
        assertEquals("u2", v.pending.value?.subject?.userId)

        v.refresh(null) // signed out
        assertNull(v.pending.value)
        // u1 again: its earlier "That's right" was for that run with u1, which has ended.
        withRequest()
        v.refresh("u1")
        assertEquals(request, v.pending.value)
    }

    @Test fun `a request of another account is never shown`() = runTest {
        withRequest(request.copy(subject = AccountRef("u9", "mallory")))
        val v = OpenRequestViewModel(api, mockk())
        v.refresh("u1")
        assertNull(v.pending.value)
    }

    @Test fun `cancel from a full passkey session on a phone without a key`() = runTest {
        withRequest()
        api.cancelResult = request.copy(state = "cancelled")
        val passkeys = RecordingPasskeyStepUp()
        val v = OpenRequestViewModel(api, nonKeyPhoneStepUp(backgroundScope, passkeys))
        v.refresh("u1")
        v.cancel(activity)
        assertEquals(listOf("request.cancel:rq1"), passkeys.asked)
        assertTrue("cancelRequest:rq1" in api.calls)
        assertNull(v.pending.value)
    }

    @Test fun `cancel refusals say why`() = runTest {
        withRequest()
        val passkeys = RecordingPasskeyStepUp { _, _ -> Result.failure(apiError(403, "FULL_SESSION_REQUIRED")) }
        val v = OpenRequestViewModel(api, nonKeyPhoneStepUp(backgroundScope, passkeys))
        v.refresh("u1")
        v.cancel(activity)
        assertEquals(R.string.security_probation_read_only, v.message.value)
        assertEquals(request, v.pending.value)

        api.failure = apiError(401, "REQUEST_FACTOR_INVALID")
        v.cancel(activity, Part2Factor.totp("000000"))
        assertEquals(R.string.security_part2_wrong, v.message.value)
        assertFalse(v.working.value)
    }

    @Test fun `the prompt covers a chat but never a sign-in or a running ceremony`() {
        val signIn = setOf("login", "setup_flow")
        val ceremonies = setOf("scan", "approve_login")
        assertTrue(OpenRequestRules.showOver("channel/{channelId}", signIn, ceremonies, "open_request"))
        assertTrue(OpenRequestRules.showOver("home", signIn, ceremonies, "open_request"))
        assertFalse(OpenRequestRules.showOver("approve_login", signIn, ceremonies, "open_request"))
        assertFalse(OpenRequestRules.showOver("login", signIn, ceremonies, "open_request"))
        assertFalse(OpenRequestRules.showOver("open_request", signIn, ceremonies, "open_request"))
        assertFalse(OpenRequestRules.showOver(null, signIn, ceremonies, "open_request"))
    }
}
