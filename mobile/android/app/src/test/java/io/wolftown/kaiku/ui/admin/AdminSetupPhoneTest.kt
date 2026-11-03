package io.wolftown.kaiku.ui.admin

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.domain.model.Enrollment
import io.wolftown.kaiku.domain.model.EnrollmentClaimedBy
import io.wolftown.kaiku.domain.model.EnrollmentIssued
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.FakeEnrollmentApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import io.wolftown.kaiku.ui.approval.EnrollmentIssuerViewModel
import io.wolftown.kaiku.ui.approval.IssuerMode
import io.wolftown.kaiku.ui.approval.IssuerUi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableSharedFlow
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
class AdminSetupPhoneTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val admin: AdminApi = mockk(relaxed = true)
    private val stepUp: DeviceStepUp = mockk()
    private val enroll = FakeEnrollmentApi()
    private val approvals = FakeApprovalApi()
    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 8)
    private val subject = "66666666-6666-4666-8666-666666666666"
    private val enrollmentId = "55555555-5555-4555-8555-555555555555"
    private val issued = EnrollmentIssued(enrollmentId, "tok", "https://chat.example.com/setup#v=1&t=tok", "2026-11-03T12:10:00Z")

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun mode(newAccount: Boolean = false, lost: List<String> = emptyList(), m: String = "setup") =
        IssuerMode.Admin(subject, "Oma", m, "called_back", "Called her back on the landline", lost, newAccount)

    private fun TestScope.vm(): EnrollmentIssuerViewModel {
        val f = KeyDeviceFixture(backgroundScope)
        coEvery { stepUp.stepUp(any(), StepUpAction.ADMIN_ENROLLMENT_ISSUE, subject, any()) } returns
            StepUpOutcome.Granted(StepUpGrant("admin.enrollment.issue", subject, "device", "t"))
        coEvery { admin.issueEnrollment(any(), any(), any(), any(), any()) } returns issued
        approvals.nextChallenge = SigFixtures.challengeFor(SigKind2.ENROLL, 2, SigFixtures.V02_CONTEXT)
        return EnrollmentIssuerViewModel(stepUp, enroll.api, approvals, f.signer, f.repo, { SigFixtures.DEVICE_ID }, { true }, events,
            issueAdmin = { m -> admin.issueEnrollment(m.userId, m.mode, m.claimantMethod, m.note, m.lostDeviceIds) })
    }

    @Test fun `setup records the claimant check and note`() = runTest {
        vm().start(activity, mode())
        coVerify { admin.issueEnrollment(subject, "setup", "called_back", "Called her back on the landline", emptyList()) }
    }

    @Test fun `each setup code needs its own step-up`() = runTest {
        val a = vm()
        a.start(activity, mode())
        a.cancel()
        val b = vm()
        b.start(activity, mode())
        coVerify(exactly = 2) { stepUp.stepUp(activity, StepUpAction.ADMIN_ENROLLMENT_ISSUE, subject, any()) }
        coVerify(exactly = 2) { admin.issueEnrollment(any(), any(), any(), any(), any()) }
    }

    @Test fun `recover mode sends the chosen lost devices`() = runTest {
        vm().start(activity, mode(m = "recovery", lost = listOf("kd1")))
        coVerify { admin.issueEnrollment(subject, "recovery", "called_back", any(), listOf("kd1")) }
    }

    @Test fun `no independent canceller shows the cli hint`() = runTest {
        val v = vm()
        coEvery { admin.issueEnrollment(any(), any(), any(), any(), any()) } throws apiError(409, "NO_INDEPENDENT_CANCELLER")
        v.start(activity, mode())
        assertEquals(IssuerUi.Ended(R.string.admin_no_canceller), v.ui.value)
    }

    @Test fun `existing account done shows the ready time`() = runTest {
        val v = vm()
        v.start(activity, mode(newAccount = false))
        enroll.enrollment = Enrollment(enrollmentId, "admin", EnrollmentState.CLAIMED, AccountRef(subject, "oma", "Oma"),
            claimed = EnrollmentClaimedBy("key_device", "Pixel 8"), triesLeft = 3)
        events.emit(ServerEvent.EnrollmentUpdated(enrollmentId, "claimed"))
        v.onDigits("123456")
        approvals.answerResult = AnswerResult("consumed")
        v.confirmCode(activity)
        val done = v.ui.value as IssuerUi.Done
        assertTrue(done.pending != null)
    }

    @Test fun `claimant check note must be 1 to 500 characters`() {
        assertEquals(false, ClaimantCheck.noteValid(""))
        assertEquals(true, ClaimantCheck.noteValid("In person at the kitchen table"))
        assertEquals(false, ClaimantCheck.noteValid("x".repeat(501)))
    }
}

private typealias SigKind2 = io.wolftown.kaiku.approval.SigKind
