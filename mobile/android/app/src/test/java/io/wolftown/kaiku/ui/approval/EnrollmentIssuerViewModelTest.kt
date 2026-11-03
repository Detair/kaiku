package io.wolftown.kaiku.ui.approval

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.KaikuSig
import io.wolftown.kaiku.approval.SigContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.AnswerResult
import io.wolftown.kaiku.domain.model.Enrollment
import io.wolftown.kaiku.domain.model.EnrollmentClaimedBy
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.FakeEnrollmentApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
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
class EnrollmentIssuerViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val stepUp: DeviceStepUp = mockk()
    private val enroll = FakeEnrollmentApi()
    private val approvals = FakeApprovalApi()
    private val events = MutableSharedFlow<ServerEvent>(extraBufferCapacity = 8)
    private val enrollmentId = "55555555-5555-4555-8555-555555555555"
    private var mayApprove: Boolean? = true

    /** The issuer's enroll challenge for the typed digits, built as the server would (V02 shape, own variant). */
    private fun enrollChallenge(label: String = "Pixel 8") = SigFixtures.challengeFor(
        SigKind.ENROLL, 2,
        """{"enrollment_id":"$enrollmentId","subject_id":"${SigFixtures.USER_ID}","subject_username":"alice","variant":"own","authenticator_type":"key_device","new_key_hash":"Bs5H8qw80BPj7K2nOZOOcMYlOZs6KhOHt1xIDKHM_9Q","label":"$label","claimant_check":"none"}""",
    )

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.vm(): Pair<EnrollmentIssuerViewModel, KeyDeviceFixture> {
        val f = KeyDeviceFixture(backgroundScope)
        coEvery { stepUp.stepUp(any(), StepUpAction.ENROLLMENT_ISSUE, "", any()) } returns
            StepUpOutcome.Granted(StepUpGrant("enrollment.issue", "", "device", "t"))
        enroll.issued = enroll.issued.copy(enrollmentId = enrollmentId)
        enroll.enrollment = Enrollment(enrollmentId, "own", EnrollmentState.ISSUED, AccountRef(SigFixtures.USER_ID, "alice", "Alice"))
        approvals.nextChallenge = enrollChallenge()
        val v = EnrollmentIssuerViewModel(
            stepUp, enroll.api, approvals, f.signer, f.repo, { SigFixtures.DEVICE_ID }, { mayApprove }, events,
            pollMs = 3_000L,
        )
        return v to f
    }

    private fun claimed(label: String = "Pixel 8", tries: Int = 3) = Enrollment(
        enrollmentId, "own", EnrollmentState.CLAIMED, AccountRef(SigFixtures.USER_ID, "alice", "Alice"),
        claimed = EnrollmentClaimedBy("key_device", label), triesLeft = tries,
    )

    @Test fun `own flow steps up before issuing`() = runTest {
        val (v, _) = vm()
        v.start(activity, IssuerMode.Own)
        coVerify(ordering = io.mockk.Ordering.ORDERED) { stepUp.stepUp(activity, StepUpAction.ENROLLMENT_ISSUE, "", any()) }
        assertTrue(enroll.calls.first() == "issueOwn")
        assertEquals(IssuerUi.ShowSetup(enroll.issued.setupUrl, java.time.Instant.parse(enroll.issued.expiresAt)), v.ui.value)
    }

    @Test fun `no grant issues nothing`() = runTest {
        val (v, _) = vm()
        coEvery { stepUp.stepUp(any(), any(), any(), any()) } returns StepUpOutcome.Cancelled
        v.start(activity, IssuerMode.Own)
        assertTrue(enroll.calls.isEmpty())
        assertEquals(IssuerUi.Ended(R.string.issuer_stopped), v.ui.value)
    }

    @Test fun `may approve off offers no add a phone`() = runTest {
        mayApprove = false
        val (v, _) = vm()
        assertEquals(false, canAddPhone(mayApprove = false, isKeyDevice = true))
        assertEquals(false, canAddPhone(mayApprove = true, isKeyDevice = false))
        assertEquals(true, canAddPhone(mayApprove = true, isKeyDevice = true))
        v.start(activity, IssuerMode.Own)
        assertEquals(IssuerUi.Ended(R.string.scan_may_approve_off), v.ui.value)
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any(), any()) }
    }

    @Test fun `claimed shows the device calling itself`() = runTest {
        val (v, _) = vm()
        v.start(activity, IssuerMode.Own)
        enroll.enrollment = claimed()
        events.emit(ServerEvent.EnrollmentUpdated(enrollmentId, "claimed"))
        assertEquals(IssuerUi.TypeCode("Pixel 8", "", 3), v.ui.value)
        assertTrue(enroll.calls.contains("get:$enrollmentId"))
    }

    @Test fun `claim is also found by polling`() = runTest {
        val (v, _) = vm()
        v.start(activity, IssuerMode.Own)
        enroll.enrollment = claimed()
        testScheduler.advanceTimeBy(3_100)
        assertEquals(IssuerUi.TypeCode("Pixel 8", "", 3), v.ui.value)
    }

    @Test fun `confirm signs enroll with the typed digits`() = runTest {
        val (v, f) = vm()
        v.start(activity, IssuerMode.Own)
        enroll.enrollment = claimed()
        events.emit(ServerEvent.EnrollmentUpdated(enrollmentId, "claimed"))
        v.onDigits("123456")
        approvals.answerResult = AnswerResult("consumed", enrollment = claimed().copy(state = EnrollmentState.DONE))
        v.confirmCode(activity)
        assertEquals("enrollmentChallenge:$enrollmentId", approvals.calls.first())
        val (decision, response, signature) = approvals.lastAnswer!!
        assertEquals("approve", decision)
        assertEquals(SigFixtures.b64u("123456".toByteArray(Charsets.US_ASCII)), response)
        val m = KaikuSig.message(SigFixtures.unb64u(approvals.nextChallenge!!.signingPrefix), io.wolftown.kaiku.approval.Decision.APPROVE, "123456".toByteArray(Charsets.US_ASCII))
        assertTrue(SigFixtures.verifyP256(SigFixtures.b64u(f.keyStore.publicKeySpki()!!), m, signature))
        assertEquals(IssuerUi.Done("Pixel 8", null), v.ui.value)
    }

    @Test fun `confirm needs six digits`() = runTest {
        val (v, _) = vm()
        v.start(activity, IssuerMode.Own)
        enroll.enrollment = claimed()
        events.emit(ServerEvent.EnrollmentUpdated(enrollmentId, "claimed"))
        v.onDigits("12345")
        v.confirmCode(activity)
        assertTrue(approvals.calls.isEmpty())
    }

    @Test fun `a label that differs from the signed one is refused`() = runTest {
        val (v, f) = vm()
        approvals.nextChallenge = enrollChallenge(label = "Evil phone")
        v.start(activity, IssuerMode.Own)
        enroll.enrollment = claimed()
        events.emit(ServerEvent.EnrollmentUpdated(enrollmentId, "claimed"))
        v.onDigits("123456")
        v.confirmCode(activity)
        assertEquals(IssuerUi.Ended(R.string.approval_mismatch), v.ui.value)
        assertTrue(f.verifier.prompts.isEmpty())
    }

    @Test fun `wrong code keeps the screen with tries left`() = runTest {
        val (v, _) = vm()
        v.start(activity, IssuerMode.Own)
        enroll.enrollment = claimed()
        events.emit(ServerEvent.EnrollmentUpdated(enrollmentId, "claimed"))
        v.onDigits("123456")
        approvals.answerFailure = apiError(400, "ENROLL_CODE_WRONG", ApiErrorFields(triesLeft = 2))
        v.confirmCode(activity)
        assertEquals(IssuerUi.TypeCode("Pixel 8", "", 2, wrong = true), v.ui.value)
        // The next confirm asks for a fresh challenge.
        v.onDigits("123457")
        approvals.answerFailure = null
        approvals.answerResult = AnswerResult("consumed")
        v.confirmCode(activity)
        assertEquals(2, approvals.calls.count { it.startsWith("enrollmentChallenge") })
    }

    @Test fun `third wrong code ends the enrolment`() = runTest {
        val (v, _) = vm()
        v.start(activity, IssuerMode.Own)
        enroll.enrollment = claimed(tries = 1)
        events.emit(ServerEvent.EnrollmentUpdated(enrollmentId, "claimed"))
        v.onDigits("000000")
        approvals.answerFailure = apiError(410, "ENROLLMENT_FAILED")
        v.confirmCode(activity)
        assertEquals(IssuerUi.Ended(R.string.issuer_three_wrong), v.ui.value)
    }

    @Test fun `cancel deletes the enrolment`() = runTest {
        val (v, _) = vm()
        v.start(activity, IssuerMode.Own)
        v.cancel()
        assertTrue(enroll.calls.contains("cancel:$enrollmentId"))
        assertEquals(IssuerUi.Ended(R.string.issuer_stopped), v.ui.value)
    }
}
