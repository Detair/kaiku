package io.wolftown.kaiku.ui.security

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceStepUp
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
    private val stepUp: DeviceStepUp = mockk()
    private val me = "11111111-1111-4111-8111-111111111111"
    private val vm by lazy { NoticeActionsViewModel(stepUp, security, approvals, admin) { me } }

    private fun event(vararg actions: EventAction) = SecurityEvent(
        id = "ev1", eventType = "sign_in", createdAt = "2026-11-03T12:00:00Z", subjectId = me,
        authenticator = AuthenticatorRef("kd9", "key_device", "Stolen phone"), requestId = "rq1",
        actions = actions.toList(),
    )

    private fun notice(subject: String = me) = SecurityNotice("n1", AccountRef(subject, "oma", "Oma"), "sign_in")

    private fun grant(action: StepUpAction, target: String) {
        coEvery { stepUp.stepUp(activity, action, target, any()) } returns StepUpOutcome.Granted(StepUpGrant(action.wire, target, "device", "t"))
    }

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    @Test fun `repudiate shows the warning before stepping up`() = runTest {
        grant(StepUpAction.EVENT_REPUDIATE, "ev1")
        security.repudiateResult = RepudiateResult(event(), compromised = listOf("password"))
        val a = EventAction(NoticeAction.REPUDIATE, "step_up", "voids_own_request")
        vm.run(activity, a, event(a), notice())
        assertEquals(NoticeActionUi.Warning(R.string.security_warning_voids_own_request), vm.ui.value)
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any(), any()) }
        vm.confirmWarning(activity)
        coVerify { stepUp.stepUp(activity, StepUpAction.EVENT_REPUDIATE, "ev1", any()) }
        assertTrue("repudiate:ev1" in security.calls)
        assertEquals(NoticeActionUi.Result(R.string.security_repudiated_password, offerRemoveDevice = false), vm.ui.value)
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
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any(), any()) }
        assertEquals(NoticeActionUi.Result(R.string.security_request_cancelled, offerRemoveDevice = false), vm.ui.value)
    }

    @Test fun `a cancel that lost against completion offers remove this device`() = runTest {
        grant(StepUpAction.REQUEST_CANCEL, "rq1")
        security.failure = apiError(409, "ACCOUNT_REQUEST_COMPLETED")
        val a = EventAction(NoticeAction.CANCEL_REQUEST, "step_up", null)
        vm.run(activity, a, event(a), notice())
        assertEquals(NoticeActionUi.Result(R.string.security_request_too_late, offerRemoveDevice = true), vm.ui.value)
    }

    @Test fun `admin reader removes with the admin action`() = runTest {
        grant(StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, "kd9")
        val other = "66666666-6666-4666-8666-666666666666"
        val a = EventAction(NoticeAction.REMOVE_DEVICE, "step_up", null)
        vm.run(activity, a, event(a).copy(subjectId = other), notice(subject = other))
        coVerify { stepUp.stepUp(activity, StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, "kd9", "Stolen phone") }
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
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any(), any()) }
    }
}
