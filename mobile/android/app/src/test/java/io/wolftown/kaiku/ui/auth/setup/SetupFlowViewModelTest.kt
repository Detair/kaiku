package io.wolftown.kaiku.ui.auth.setup

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceSuitabilityChecker
import io.wolftown.kaiku.approval.UnsuitableReason
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.domain.model.ClaimResult
import io.wolftown.kaiku.domain.model.ClaimStatus
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.testutil.FakeEnrollmentApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
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
import java.time.Instant

@OptIn(ExperimentalCoroutinesApi::class)
class SetupFlowViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val token = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
    private val entry = SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.SCAN)
    private val enroll = FakeEnrollmentApi()
    private var decision: UvDecision = UvPolicy.choose(30, true, true)
    private val suitability = object : DeviceSuitabilityChecker { override fun check() = decision }
    private val deviceSignIn: DeviceSignIn = mockk()
    private val env = FakeSetupEnvironment()

    /** Runs inline but marks that key work went through the key dispatcher (never Main). */
    private var onKeyDispatcher = false
    private val keyDispatcher = object : kotlinx.coroutines.CoroutineDispatcher() {
        override fun dispatch(context: kotlin.coroutines.CoroutineContext, block: Runnable) {
            onKeyDispatcher = true
            try { block.run() } finally { onKeyDispatcher = false }
        }
    }

    class FakeSetupEnvironment : SetupEnvironment {
        var pinned: String? = null
        var signedInUser: String? = null
        val saved = mutableListOf<String>()
        override fun pinnedOrigin() = pinned
        override fun signedInUserId() = signedInUser
        override fun saveServerOrigin(origin: String) { saved += origin }
        override val deviceLabel = "Google Pixel 8"
        override val apiLevel = 35
    }

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.vm(): Pair<SetupFlowViewModel, KeyDeviceFixture> {
        // A fresh phone: no key device, no key, signed out.
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.keyStore.delete()
        f.authState.setLoggedOut()
        coEvery { deviceSignIn.signIn(any()) } returns DeviceSignInResult.Success
        val v = SetupFlowViewModel(enroll.api, suitability, f.keyStore, f.signer, f.repo, { SigFixtures.DEVICE_ID }, deviceSignIn, env,
            keyDispatcher = keyDispatcher)
        return v to f
    }

    @Test fun `setup for another server is refused before any request`() = runTest {
        env.pinned = "https://chat.example.com"
        val (v, _) = vm()
        v.start(entry.copy(origin = "https://evil.example.com"))
        assertEquals(SetupUi.Ended(R.string.setup_other_server, canRetry = false), v.ui.value)
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `phone that is already a key refuses another setup`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val f = KeyDeviceFixture(backgroundScope)
        val v = SetupFlowViewModel(enroll.api, suitability, f.keyStore, f.signer, f.repo, { SigFixtures.DEVICE_ID }, deviceSignIn, env,
            keyDispatcher = keyDispatcher)
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_already_key, canRetry = false), v.ui.value)
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `unconfigured app shows the server name before the subject`() = runTest {
        val (v, _) = vm()
        v.start(entry.copy(origin = "https://other.example.org"))
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "other.example.org"), v.ui.value)
        assertEquals(listOf("open"), enroll.calls)
        assertEquals("https://other.example.org", enroll.lastOrigin)
        assertTrue(env.saved.isEmpty())
        v.confirmServer()
        assertEquals(listOf("https://other.example.org"), env.saved)
        assertTrue(enroll.calls.contains("popChallenge"))
    }

    @Test fun `no screen lock shows the guide and rechecks on resume`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        decision = UvDecision.Unsuitable(UnsuitableReason.NO_SCREEN_LOCK)
        val (v, f) = vm()
        v.start(entry)
        assertEquals(SetupUi.ScreenLockNeeded(UnsuitableReason.NO_SCREEN_LOCK), v.ui.value)
        assertFalse(f.keyStore.hasKey())
        assertFalse(enroll.calls.contains("popChallenge"))
        decision = UvPolicy.choose(30, true, true)
        v.recheckScreenLock()
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller"), v.ui.value)
    }

    @Test fun `phone without a hardware keystore cannot be a key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        decision = UvDecision.Unsuitable(UnsuitableReason.NO_HARDWARE_KEYSTORE)
        val (v, _) = vm()
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_cannot_be_key, canRetry = false), v.ui.value)
    }

    @Test fun `setup for another user while signed in asks to sign out`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        env.signedInUser = SigFixtures.USER_ID
        val (v, f) = vm()
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_for_someone_else, canRetry = false, arg = "Oma"), v.ui.value)
        assertFalse(f.keyStore.hasKey())
        assertFalse(enroll.calls.contains("claim"))
    }

    @Test fun `claim sends attestation chain uv class and label`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        v.start(entry)
        assertTrue(f.keyStore.generatedChallenges.last().contentEquals(enroll.nonce))
        v.confirmSubject(activity)
        val c = enroll.lastClaim!!
        assertEquals(token, c.setupToken)
        assertEquals(SigFixtures.challengeId(5), c.challengeId)
        assertEquals(listOf(SigFixtures.b64u(byteArrayOf(1, 2, 3)), SigFixtures.b64u(byteArrayOf(4, 5))), c.attestationChain)
        assertEquals("per_use", c.uvClass)
        assertEquals("Google Pixel 8", c.label)
        assertEquals(35, c.apiLevel)
        assertEquals(SigFixtures.b64u(f.keyStore.publicKeySpki()!!), c.publicKey)
        assertEquals(R.string.security_uv_title, f.verifier.prompts.single().title)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals("claim-secret", f.repo.local()!!.claimSecret)
    }

    @Test fun `code is shown grouped`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, _) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        val ui = v.ui.value as SetupUi.ShowCode
        assertEquals("123 456", ui.code)
        assertEquals(Instant.parse("2026-11-03T12:05:00Z"), ui.expiresAt)
    }

    @Test fun `active result signs in with the device key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        val (v, _) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        v.signIn(activity)
        coVerify { deviceSignIn.signIn(activity) }
        assertEquals(SetupUi.Done, v.ui.value)
    }

    @Test fun `pending result keeps the key and shows the ready time`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "pending", "2026-11-04T12:00:00Z")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertEquals(SetupUi.Pending(Instant.parse("2026-11-04T12:00:00Z")), v.ui.value)
        assertEquals(KeyDeviceState.Pending(Instant.parse("2026-11-04T12:00:00Z"), "e1".let { f.repo.local()!!.enrollmentId }), f.repo.current())
    }

    @Test fun `cancelled before confirmation forgets the key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.CANCELLED)
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_cancelled, canRetry = false), v.ui.value)
        assertFalse(f.keyStore.hasKey())
        assertNull(f.repo.local())
    }

    @Test fun `already claimed shows ask for a new code`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals(SetupUi.Ended(R.string.setup_already_used, canRetry = false), v.ui.value)
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `register binds this phone with the ticket and no code`() = runTest {
        env.pinned = null
        enroll.claimResult = ClaimResult("55555555-5555-4555-8555-555555555555", "done", authenticatorState = "active")
        val (v, _) = vm()
        v.start(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.TICKET, ticket = "ticket-secret"))
        // A ticket comes from this app's own registration: no server question.
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller"), v.ui.value)
        v.confirmSubject(activity)
        assertEquals("ticket-secret", enroll.lastTicket)
        assertFalse(enroll.calls.contains("status"))
        assertEquals(SetupUi.SigningIn, v.ui.value)
    }

    @Test fun `a server hiccup during the code wait keeps the key and polls again`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statusFailures += apiError(503, "SERVICE_UNAVAILABLE")
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        advanceTimeBy(5_000)
        runCurrent()
        assertTrue(f.keyStore.hasKey())
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(2, enroll.calls.count { it == "status" })
    }

    @Test fun `an unknown enrolment during the code wait ends the setup`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statusFailures += apiError(404, "ENROLLMENT_TOKEN_INVALID")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertFalse(f.keyStore.hasKey())
        assertTrue(v.ui.value is SetupUi.Ended)
    }

    @Test fun `a new code after an ended setup starts again`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, _) = vm()
        v.start(entry.copy(origin = "https://evil.example.com"))
        assertTrue(v.ui.value is SetupUi.Ended)
        v.reset()
        assertFalse(v.started)
        v.start(entry)
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller"), v.ui.value)
    }

    @Test fun `key work runs off the main thread`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        var generatedOffMain = false
        f.keyStore.onGenerate = { generatedOffMain = onKeyDispatcher }
        v.start(entry)
        assertTrue(generatedOffMain)
    }
}
