package io.wolftown.kaiku.ui.auth.setup

import io.wolftown.kaiku.testutil.seed
import io.wolftown.kaiku.data.keydevice.StoredKeyDevice
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.testutil.saveClaimed
import io.wolftown.kaiku.testutil.saveClaiming
import io.wolftown.kaiku.testutil.markEnrolled
import io.wolftown.kaiku.testutil.markPending
import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.domain.model.ClaimStatus
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.testutil.FakeEnrollmentApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.time.Instant

@OptIn(ExperimentalCoroutinesApi::class)
class SetupPendingViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val enroll = FakeEnrollmentApi()
    private val deviceSignIn: DeviceSignIn = mockk()
    private val readyAt = Instant.parse("2026-11-04T12:00:00Z")
    private val approval = FakeApprovalApi()
    private val probe = KeyDeviceProbe(approval) { SigFixtures.DEVICE_ID }

    private fun pendingVm(repo: KeyDeviceRepository) =
        SetupPendingViewModel(setupOwner(enroll.api, repo, mockk(), deviceSignIn, probe))

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun pendingDevice() = LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE,
        "55555555-5555-4555-8555-555555555555", "claim-secret", "Oma")

    @Test fun `pending setup resumes at app start and signs in when active`() = runTest {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        val store = InMemoryKeyDeviceStore()
        KeyDeviceRepository(store, f.keyStore, f.authState, backgroundScope).markPending(pendingDevice(), readyAt)
        // App restart: a new repository instance reads the pending setup.
        val repo = KeyDeviceRepository(store, f.keyStore, f.authState, backgroundScope)
        assertEquals(KeyDeviceState.Pending(readyAt, "55555555-5555-4555-8555-555555555555"), repo.current())
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        // A pending key device asks the key-device probe (the finished enrolment ages out, 30 days).
        approval.failWith = apiError(409, "KEY_DEVICE_PENDING", ApiErrorFields(readyAt = readyAt.toString()))

        val v = pendingVm(repo)
        v.check()
        assertEquals(SetupUi.Pending(readyAt), v.ui.value)
        approval.failWith = null
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        v.check()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertFalse(enroll.calls.contains("status"))
        // Always the key's own server.
        assertEquals(SigFixtures.ORIGIN, approval.deviceLoginOrigin)
        v.signIn(activity)
        coVerify { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.AFTER_SETUP) }
        assertEquals(SetupUi.Done, v.ui.value)
    }

    @Test fun `cancelled pending setup shows the cancelled screen and forgets the key`() = runTest {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), f.keyStore, f.authState, backgroundScope)
        repo.markPending(pendingDevice(), readyAt)
        approval.failWith = apiError(403, "KEY_DEVICE_REVOKED")

        val v = pendingVm(repo)
        v.check()
        assertEquals(SetupUi.Ended(R.string.setup_cancelled, canRetry = false), v.ui.value)
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(f.keyStore.hasKey())
    }

    private fun TestScope.repo(): Pair<KeyDeviceRepository, KeyDeviceFixture> {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        return KeyDeviceRepository(InMemoryKeyDeviceStore(), f.keyStore, f.authState, backgroundScope) to f
    }

    @Test fun `an active setup that has not signed in yet signs in at app start`() = runTest {
        val (repo, _) = repo()
        repo.markEnrolled(pendingDevice().copy(claimSecret = null))
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success

        val v = pendingVm(repo)
        assertEquals(SetupUi.SigningIn, v.ui.value)
        v.signIn(activity)
        assertEquals(SetupUi.Done, v.ui.value)
    }

    @Test fun `a resumed setup explains its second prompt and a slow one offers a fresh try`() = runTest {
        val (repo, _) = repo()
        repo.markEnrolled(pendingDevice().copy(claimSecret = null))
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Expired
        val v = pendingVm(repo)
        v.signIn(activity)
        coVerify(exactly = 1) { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.AFTER_SETUP) }
        assertEquals(SetupUi.SignInTimedOut, v.ui.value)
        v.check() // a resume does not prompt by itself
        assertEquals(SetupUi.SignInTimedOut, v.ui.value)
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        v.signIn(activity)
        assertEquals(SetupUi.Done, v.ui.value)
    }

    @Test fun `a resumed final sign-in that never ends offers a fresh try after a while`() = runTest {
        val (repo, _) = repo()
        repo.markEnrolled(pendingDevice().copy(claimSecret = null))
        coEvery { deviceSignIn.signIn(any(), any()) } coAnswers { kotlinx.coroutines.awaitCancellation() }
        val v = pendingVm(repo)
        v.signIn(activity)
        assertEquals(SetupUi.SigningIn, v.ui.value)
        advanceTimeBy(SetupFlowViewModel.SIGN_IN_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.SignInTimedOut, v.ui.value)
    }

    @Test fun `a claim sent before the app closed is reconciled at app start`() = runTest {
        val (repo, f) = repo()
        repo.saveClaiming(pendingDevice().copy(claimSecret = null))
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)

        val v = pendingVm(repo)
        v.check()

        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertTrue(f.keyStore.hasKey())
        assertFalse(enroll.calls.contains("status"))
    }

    /**
     * Fable v1 final check, minor 4: a 404 from the enrolment status may only
     * mean the finished enrolment was swept (30 days); the key device decides.
     */
    @Test fun `a swept enrolment of a confirmed key keeps the key`() = runTest {
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statusFailures += apiError(404, "ENROLLMENT_TOKEN_INVALID")
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)

        val v = pendingVm(repo)
        v.check()

        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `a 404 that is not the server's own answer for an unknown enrolment decides nothing`() = runTest {
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statusFailures += apiError(404, "NOT_FOUND")
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")

        val v = pendingVm(repo)
        v.check()

        assertTrue(approval.calls.isEmpty())
        assertEquals(SetupUi.WaitingForCode, v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMED, repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `an ending state with another status than 200 decides nothing`() = runTest {
        // What the client's API layer does with it is its own test; here: the answer as an error.
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statusFailures += io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.Accepted, "status failed")
        val v = pendingVm(repo)
        v.check()
        assertEquals(KeyDevicePhase.CLAIMED, repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `an ended enrolment forgets the claim it was asked for`() = runTest {
        for ((state, text) in listOf(
            EnrollmentState.EXPIRED to R.string.setup_expired,
            EnrollmentState.CANCELLED to R.string.setup_cancelled,
            EnrollmentState.FAILED to R.string.setup_code_failed,
        )) {
            val (repo, f) = repo()
            repo.saveClaimed(pendingDevice())
            enroll.statuses.clear()
            enroll.statuses += ClaimStatus(state)
            enroll.statuses += ClaimStatus(state) // a code that ran out is asked about twice
            val v = pendingVm(repo)
            v.check()
            advanceTimeBy(SetupOwner.EXPIRED_RECHECK_MS + 1)
            runCurrent()
            assertEquals(SetupUi.Ended(text, canRetry = false), v.ui.value)
            assertEquals(KeyDeviceState.None, repo.current())
            assertFalse(f.keyStore.hasKey())
        }
    }

    @Test fun `a code that ran out counts once it is said again, a confirmation stored meanwhile wins`() = runTest {
        // The code's time is just over while the confirmation typed in time is still being stored.
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statuses += ClaimStatus(EnrollmentState.EXPIRED)
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        val v = pendingVm(repo)
        v.check()
        // Said once: nothing is forgotten yet.
        assertEquals(SetupUi.WaitingForCode, v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMED, repo.phase())
        assertTrue(f.keyStore.hasKey())
        advanceTimeBy(SetupOwner.EXPIRED_RECHECK_MS + 1)
        runCurrent()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `a late ending answer for an older claim never costs the claim stored now`() = runTest {
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        val held = kotlinx.coroutines.CompletableDeferred<Unit>()
        enroll.statusGate = held
        enroll.statuses += ClaimStatus(EnrollmentState.CANCELLED)
        val v = pendingVm(repo)
        v.check()
        // While the status request is under way, the phone is set up anew: another enrolment, another key.
        f.keyStore.delete()
        val newKey = f.keyStore.generate(ByteArray(32), io.wolftown.kaiku.approval.UvPolicy.choose(30, true, true) as io.wolftown.kaiku.approval.UvDecision.Use).keyId
        repo.saveClaimed(pendingDevice().copy(enrollmentId = "66666666-6666-4666-8666-000000000002"))
        held.complete(Unit)
        runCurrent()
        assertEquals("66666666-6666-4666-8666-000000000002", repo.local()!!.enrollmentId)
        assertEquals(newKey, f.keyStore.keyId())
    }

    @Test fun `an unknown enrolment without a key device forgets the key`() = runTest {
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statusFailures += apiError(404, "ENROLLMENT_TOKEN_INVALID")
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")

        val v = pendingVm(repo)
        v.check()

        assertEquals(SetupUi.Ended(R.string.setup_cancelled, canRetry = false), v.ui.value)
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `a gone enrolment while the server is unreachable keeps the key`() = runTest {
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statusFailures += apiError(410, "ENROLLMENT_FAILED")
        approval.failWith = java.io.IOException("offline")

        val v = pendingVm(repo)
        v.check()

        assertTrue(f.keyStore.hasKey())
        assertEquals(KeyDevicePhase.CLAIMED, repo.phase())
    }

    private val claimTime: Instant = Instant.parse("2026-11-03T12:10:00Z")

    private fun noKeyDeviceAt(t: Instant?) = apiError(404, "KEY_DEVICE_UNKNOWN", ApiErrorFields(serverTime = t))

    @Test fun `a claim that never landed is dropped at app start once its time has passed`() = runTest {
        val (repo, f) = repo()
        repo.seed(StoredKeyDevice(pendingDevice().copy(claimSecret = null), KeyDevicePhase.CLAIMING, claimVoidAfter = claimTime, codeClaim = true))
        approval.failWith = noKeyDeviceAt(claimTime.plusSeconds(1))

        val v = pendingVm(repo)
        v.check()

        assertEquals(SetupUi.Ended(R.string.setup_not_finished, canRetry = false), v.ui.value)
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `a claim whose answer never came is kept while the server could still store it`() = runTest {
        val (repo, f) = repo()
        repo.seed(StoredKeyDevice(pendingDevice().copy(claimSecret = null), KeyDevicePhase.CLAIMING, claimVoidAfter = claimTime))
        // The server knows no key device yet, three minutes before the claim's time is over.
        approval.failWith = noKeyDeviceAt(claimTime.minusSeconds(180))

        val v = pendingVm(repo)
        v.check()

        assertEquals(SetupUi.ClaimWaiting, v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, repo.phase())
        assertTrue(f.keyStore.hasKey())
        // The claim did land after all: the phone asks again by itself and goes on.
        approval.failWith = null
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        advanceTimeBy(180_000 + 6_000)
        runCurrent()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `a kept claim that never became a key is dropped by the next ask after its time`() = runTest {
        val (repo, f) = repo()
        repo.seed(StoredKeyDevice(pendingDevice().copy(claimSecret = null), KeyDevicePhase.CLAIMING, claimVoidAfter = claimTime, codeClaim = true))
        approval.failWith = noKeyDeviceAt(claimTime.minusSeconds(60))
        val v = pendingVm(repo)
        v.check()
        assertEquals(SetupUi.ClaimWaiting, v.ui.value)
        approval.failWith = noKeyDeviceAt(claimTime.plusSeconds(6))
        advanceTimeBy(60_000 + 6_000)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_not_finished, canRetry = false), v.ui.value)
        assertFalse(f.keyStore.hasKey())
    }

    /** "Try again" asks at once, also while the screen only waits to ask again by itself. */
    @Test fun `try again while the phone waits to ask again asks at once`() = runTest {
        val (repo, _) = repo()
        repo.seed(StoredKeyDevice(pendingDevice().copy(claimSecret = null), KeyDevicePhase.CLAIMING, claimVoidAfter = claimTime))
        approval.failWith = noKeyDeviceAt(claimTime.minusSeconds(600))
        val v = pendingVm(repo)
        v.check()
        assertEquals(SetupUi.ClaimWaiting, v.ui.value)
        val asked = approval.calls.size
        // The claim did land meanwhile.
        approval.failWith = null
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        v.check() // no time has passed
        assertEquals(asked + 1, approval.calls.size)
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        // The wait it replaced asks nothing more.
        advanceTimeBy(700_000)
        runCurrent()
        assertEquals(asked + 1, approval.calls.size)
    }

    @Test fun `without the server's time a claim is not given up`() = runTest {
        val (repo, f) = repo()
        repo.seed(StoredKeyDevice(pendingDevice().copy(claimSecret = null), KeyDevicePhase.CLAIMING, claimVoidAfter = claimTime))
        approval.failWith = noKeyDeviceAt(null)
        val v = pendingVm(repo)
        v.check()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `offline at app start keeps the claim and checks again`() = runTest {
        val (repo, f) = repo()
        repo.saveClaiming(pendingDevice().copy(claimSecret = null))
        approval.failWith = java.io.IOException("offline")

        val v = pendingVm(repo)
        v.check()
        assertEquals(SetupUi.Ended(R.string.security_network_error, canRetry = true), v.ui.value)
        assertTrue(f.keyStore.hasKey())

        approval.failWith = null
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        v.check()
        assertEquals(SetupUi.SigningIn, v.ui.value)
    }

    @Test fun `a pending setup without a claim secret keeps waiting and activates`() = runTest {
        val (repo, _) = repo()
        repo.markPending(pendingDevice().copy(claimSecret = null), readyAt)
        approval.failWith = apiError(409, "KEY_DEVICE_PENDING", ApiErrorFields(readyAt = "2026-11-05T08:00:00Z"))

        val v = pendingVm(repo)
        assertEquals(SetupUi.Pending(readyAt), v.ui.value)
        v.check()
        assertEquals(SetupUi.Pending(Instant.parse("2026-11-05T08:00:00Z")), v.ui.value)

        approval.failWith = null
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        v.check()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertFalse(enroll.calls.contains("status"))
    }

    @Test fun `a resume after a cancelled final prompt does not prompt again`() = runTest {
        val (repo, _) = repo()
        repo.markEnrolled(pendingDevice().copy(claimSecret = null))
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Cancelled
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        val v = pendingVm(repo)
        v.signIn(activity)
        assertEquals(SetupUi.SignInPaused, v.ui.value)
        v.check() // ON_RESUME after the prompt closed
        assertEquals(SetupUi.SignInPaused, v.ui.value)
        assertTrue(approval.calls.isEmpty())
        coVerify(exactly = 1) { deviceSignIn.signIn(any(), any()) }
    }

    @Test fun `a claim waiting for its code long-polls within the read timeout`() = runTest {
        val (repo, _) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statuses += ClaimStatus(EnrollmentState.CLAIMED)
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "pending", readyAt.toString())
        val v = pendingVm(repo)
        assertEquals(SetupUi.WaitingForCode, v.ui.value)
        v.check()
        assertEquals(SetupUi.Pending(readyAt), v.ui.value)
        assertEquals(2, enroll.calls.count { it == "status" })
        assertEquals(io.wolftown.kaiku.data.api.LongPoll.WAIT_SECONDS, enroll.lastStatusWait)
    }
}
