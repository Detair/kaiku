package io.wolftown.kaiku.ui.auth.setup

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
        coEvery { deviceSignIn.signIn(any()) } returns DeviceSignInResult.Success
        // A pending key device asks the key-device probe (the finished enrolment ages out, 30 days).
        approval.failWith = apiError(409, "KEY_DEVICE_PENDING", ApiErrorFields(readyAt = readyAt.toString()))

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
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
        coVerify { deviceSignIn.signIn(activity) }
        assertEquals(SetupUi.Done, v.ui.value)
    }

    @Test fun `cancelled pending setup shows the cancelled screen and forgets the key`() = runTest {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), f.keyStore, f.authState, backgroundScope)
        repo.markPending(pendingDevice(), readyAt)
        approval.failWith = apiError(403, "KEY_DEVICE_REVOKED")

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
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
        coEvery { deviceSignIn.signIn(any()) } returns DeviceSignInResult.Success

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
        assertEquals(SetupUi.SigningIn, v.ui.value)
        v.signIn(activity)
        assertEquals(SetupUi.Done, v.ui.value)
    }

    @Test fun `a claim sent before the app closed is reconciled at app start`() = runTest {
        val (repo, f) = repo()
        repo.saveClaiming(pendingDevice().copy(claimSecret = null))
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
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
        enroll.statusFailures += apiError(404, "NOT_FOUND")
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
        v.check()

        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `an unknown enrolment without a key device forgets the key`() = runTest {
        val (repo, f) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statusFailures += apiError(404, "NOT_FOUND")
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
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

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
        v.check()

        assertTrue(f.keyStore.hasKey())
        assertEquals(KeyDevicePhase.CLAIMED, repo.phase())
    }

    @Test fun `a claim that never landed is dropped at app start`() = runTest {
        val (repo, f) = repo()
        repo.saveClaiming(pendingDevice().copy(claimSecret = null))
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
        v.check()

        assertEquals(SetupUi.Ended(R.string.setup_not_finished, canRetry = false), v.ui.value)
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `offline at app start keeps the claim and checks again`() = runTest {
        val (repo, f) = repo()
        repo.saveClaiming(pendingDevice().copy(claimSecret = null))
        approval.failWith = java.io.IOException("offline")

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
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

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
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
        coEvery { deviceSignIn.signIn(any()) } returns DeviceSignInResult.Cancelled
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
        v.signIn(activity)
        assertEquals(SetupUi.SignInPaused, v.ui.value)
        v.check() // ON_RESUME after the prompt closed
        assertEquals(SetupUi.SignInPaused, v.ui.value)
        assertTrue(approval.calls.isEmpty())
        coVerify(exactly = 1) { deviceSignIn.signIn(any()) }
    }

    @Test fun `a claim waiting for its code long-polls within the read timeout`() = runTest {
        val (repo, _) = repo()
        repo.saveClaimed(pendingDevice())
        enroll.statuses += ClaimStatus(EnrollmentState.CLAIMED)
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "pending", readyAt.toString())
        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn, probe)
        assertEquals(SetupUi.WaitingForCode, v.ui.value)
        v.check()
        assertEquals(SetupUi.Pending(readyAt), v.ui.value)
        assertEquals(2, enroll.calls.count { it == "status" })
        assertEquals(io.wolftown.kaiku.data.api.LongPoll.WAIT_SECONDS, enroll.lastStatusWait)
    }
}
