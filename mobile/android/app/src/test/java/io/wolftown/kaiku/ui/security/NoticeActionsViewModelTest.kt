package io.wolftown.kaiku.ui.security

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.auth.AccountStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.AuthenticatorRef
import io.wolftown.kaiku.domain.model.EventAction
import io.wolftown.kaiku.domain.model.NoticeAction
import io.wolftown.kaiku.domain.model.Part2Factor
import io.wolftown.kaiku.domain.model.RepudiateResult
import io.wolftown.kaiku.domain.model.SecurityEvent
import io.wolftown.kaiku.domain.model.SecurityNotice
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.FakeSecurityApi
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
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
class NoticeActionsViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val security = FakeSecurityApi()
    private val approvals = FakeApprovalApi()
    private val admin: AdminApi = mockk(relaxed = true)
    private val stepUp: AccountStepUp = mockk()
    private val me = "11111111-1111-4111-8111-111111111111"
    private val vm by lazy { NoticeActionsViewModel(stepUp, security, approvals, admin) { me } }

    private fun event(vararg actions: EventAction) = SecurityEvent(
        id = "ev1", eventType = "sign_in", createdAt = "2026-11-03T12:00:00Z", subjectId = me,
        authenticator = AuthenticatorRef("kd9", "key_device", "Stolen phone"), requestId = "rq1",
        actions = actions.toList(),
    )

    private fun notice(subject: String = me) = SecurityNotice("n1", AccountRef(subject, "oma", "Oma"), "sign_in")

    private fun grant(action: StepUpAction, target: String) {
        coEvery { stepUp.stepUp(activity, action, target) } returns StepUpOutcome.Granted(StepUpGrant(action.wire, target, "device", "t"))
    }

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    @Test fun `repudiate shows the warning before stepping up`() = runTest {
        grant(StepUpAction.EVENT_REPUDIATE, "ev1")
        security.repudiateResult = RepudiateResult(event(), compromised = listOf("password"))
        val a = EventAction(NoticeAction.REPUDIATE, "step_up", "voids_own_request")
        vm.run(activity, a, event(a), notice())
        assertEquals(NoticeActionUi.Warning(R.string.security_warning_voids_own_request), vm.ui.value)
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any()) }
        vm.confirmWarning(activity)
        coVerify { stepUp.stepUp(activity, StepUpAction.EVENT_REPUDIATE, "ev1") }
        assertTrue("repudiate:ev1" in security.calls)
        assertEquals(NoticeActionUi.Result(R.string.security_repudiated_password), vm.ui.value)
    }

    @Test fun `part2 cancel sends the recovery code`() = runTest {
        security.cancelResult = AccountRequestSummary("rq1", AccountRef(me, "oma"), state = "cancelled")
        val a = EventAction(NoticeAction.CANCEL_REQUEST, "part2", null)
        vm.run(activity, a, event(a), notice())
        assertEquals(NoticeActionUi.NeedsPart2, vm.ui.value)
        vm.submitPart2(activity, Part2Factor.recoveryCode("ABCD-EFGH-IJKL-M"))
        assertTrue("cancelRequest:rq1" in security.calls)
        assertEquals("recovery_code", security.lastPart2!!.factor.type)
        assertEquals("ABCD-EFGH-IJKL-M", security.lastPart2!!.factor.code)
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any()) }
        assertEquals(NoticeActionUi.Result(R.string.security_request_cancelled), vm.ui.value)
    }

    @Test fun `a cancel that lost against completion offers remove this device`() = runTest {
        grant(StepUpAction.REQUEST_CANCEL, "rq1")
        security.failure = apiError(409, "ACCOUNT_REQUEST_COMPLETED")
        val a = EventAction(NoticeAction.CANCEL_REQUEST, "step_up", null)
        vm.run(activity, a, event(a), notice())
        val device = AuthenticatorRef("kd9", "key_device", "Stolen phone")
        assertEquals(NoticeActionUi.Result(R.string.security_request_too_late, removeDevice = device), vm.ui.value)

        // "Remove this device": the guarded removal of the device the completed request added.
        security.failure = null
        grant(StepUpAction.KEY_DEVICE_REMOVE, "kd9")
        vm.removeCompletedDevice(activity)
        coVerify { stepUp.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "kd9") }
        assertTrue("removeKeyDevice:kd9" in approvals.calls)
        assertEquals(NoticeActionUi.Result(R.string.security_device_removed), vm.ui.value)
    }

    @Test fun `too late without the device on the notice looks it up in the security log`() = runTest {
        grant(StepUpAction.REQUEST_CANCEL, "rq1")
        security.failure = apiError(409, "ACCOUNT_REQUEST_COMPLETED")
        security.pages += io.wolftown.kaiku.domain.model.CursorPage(listOf(
            SecurityEvent(id = "ev7", eventType = "request_completed", createdAt = "2026-11-04T12:00:00Z", subjectId = me,
                authenticator = AuthenticatorRef("kd7", "key_device", "New phone"), requestId = "rq1"),
        ))
        val a = EventAction(NoticeAction.CANCEL_REQUEST, "step_up", null)
        vm.run(activity, a, event(a).copy(authenticator = null), notice())
        assertEquals(
            NoticeActionUi.Result(R.string.security_request_too_late, removeDevice = AuthenticatorRef("kd7", "key_device", "New phone")),
            vm.ui.value,
        )
    }

    @Test fun `too late with no known device offers no removal`() = runTest {
        grant(StepUpAction.REQUEST_CANCEL, "rq1")
        security.failure = apiError(409, "ACCOUNT_REQUEST_COMPLETED")
        val a = EventAction(NoticeAction.CANCEL_REQUEST, "step_up", null)
        vm.run(activity, a, event(a).copy(authenticator = null), notice())
        assertEquals(NoticeActionUi.Result(R.string.security_request_too_late), vm.ui.value)
    }

    @Test fun `admin reader removes with the admin action`() = runTest {
        grant(StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, "kd9")
        val other = "66666666-6666-4666-8666-666666666666"
        val a = EventAction(NoticeAction.REMOVE_DEVICE, "step_up", null)
        vm.run(activity, a, event(a).copy(subjectId = other), notice(subject = other))
        coVerify { stepUp.stepUp(activity, StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, "kd9") }
        coVerify { admin.removeAuthenticator(other, "kd9") }
        assertTrue(approvals.calls.none { it.startsWith("removeKeyDevice") })
    }

    @Test fun `owner removes with the key device action`() = runTest {
        grant(StepUpAction.KEY_DEVICE_REMOVE, "kd9")
        val a = EventAction(NoticeAction.REMOVE_DEVICE, "step_up", null)
        vm.run(activity, a, event(a), notice())
        assertTrue("removeKeyDevice:kd9" in approvals.calls)
    }

    @Test fun `i will help confirms without a step-up`() = runTest {
        val other = "66666666-6666-4666-8666-666666666666"
        val a = EventAction(NoticeAction.CONFIRM_CONTACT, null, null)
        vm.run(activity, a, event(a).copy(subjectId = other), notice(subject = other))
        assertTrue("confirmContactFor:$other" in security.calls)
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any()) }
    }

    @Test fun `repudiate and cancel from a full passkey session on a phone without a key`() = runTest {
        val passkeys = io.wolftown.kaiku.testutil.RecordingPasskeyStepUp()
        val v = NoticeActionsViewModel(io.wolftown.kaiku.testutil.nonKeyPhoneStepUp(backgroundScope, passkeys), security, approvals, admin) { me }
        security.repudiateResult = RepudiateResult(event(), compromised = emptyList())
        val repudiate = EventAction(NoticeAction.REPUDIATE, "step_up", null)
        v.run(activity, repudiate, event(repudiate), notice())
        assertTrue("repudiate:ev1" in security.calls)

        security.cancelResult = AccountRequestSummary("rq1", AccountRef(me, "oma"), state = "cancelled")
        val cancel = EventAction(NoticeAction.CANCEL_REQUEST, "step_up", null)
        v.dismiss()
        v.run(activity, cancel, event(cancel), notice())
        assertTrue("cancelRequest:rq1" in security.calls)
        // Exactly the event and the request, each with its own grant.
        assertEquals(listOf("event.repudiate:ev1", "request.cancel:rq1"), passkeys.asked)
        assertEquals(NoticeActionUi.Result(R.string.security_request_cancelled), v.ui.value)
    }

    @Test fun `a phone with neither the key nor a passkey says so`() = runTest {
        val none = io.wolftown.kaiku.testutil.RecordingPasskeyStepUp { _, _ -> Result.failure(io.wolftown.kaiku.data.auth.PasskeyFailure.NoCredential) }
        val v = NoticeActionsViewModel(io.wolftown.kaiku.testutil.nonKeyPhoneStepUp(backgroundScope, none), security, approvals, admin) { me }
        val a = EventAction(NoticeAction.REPUDIATE, "step_up", null)
        v.run(activity, a, event(a), notice())
        assertEquals(NoticeActionUi.Error(R.string.security_step_up_none), v.ui.value)
        assertTrue(security.calls.none { it.startsWith("repudiate") })
    }

    @Test fun `without the own user id nothing goes to the admin route`() = runTest {
        val v = NoticeActionsViewModel(stepUp, security, approvals, admin) { null }
        val a = EventAction(NoticeAction.REMOVE_DEVICE, "step_up", null)
        v.run(activity, a, event(a), notice())
        assertEquals(NoticeActionUi.Error(R.string.security_sign_in_failed), v.ui.value)
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any()) }
        coVerify(exactly = 0) { admin.removeAuthenticator(any(), any()) }
    }

    @Test fun `i will help in a probation session says how to get a full one`() = runTest {
        security.failure = apiError(403, "FULL_SESSION_REQUIRED")
        val other = "66666666-6666-4666-8666-666666666666"
        val a = EventAction(NoticeAction.CONFIRM_CONTACT, null, null)
        vm.run(activity, a, event(a).copy(subjectId = other), notice(subject = other))
        assertEquals(NoticeActionUi.Error(R.string.security_probation_read_only), vm.ui.value)
    }

    @Test fun `a second tap while an action runs starts nothing`() = runTest {
        val gate = kotlinx.coroutines.CompletableDeferred<StepUpOutcome>()
        coEvery { stepUp.stepUp(activity, StepUpAction.EVENT_REPUDIATE, "ev1") } coAnswers { gate.await() }
        val a = EventAction(NoticeAction.REPUDIATE, "step_up", null)
        vm.run(activity, a, event(a), notice())
        assertEquals(NoticeActionUi.Working, vm.ui.value)
        vm.run(activity, a, event(a), notice())
        coVerify(exactly = 1) { stepUp.stepUp(any(), any(), any()) }
        gate.complete(StepUpOutcome.Cancelled)
    }
}
