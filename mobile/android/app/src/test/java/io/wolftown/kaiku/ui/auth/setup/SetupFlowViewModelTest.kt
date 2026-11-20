package io.wolftown.kaiku.ui.auth.setup

import io.wolftown.kaiku.testutil.saveClaimed
import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.DeviceSuitabilityChecker
import io.wolftown.kaiku.approval.UnsuitableReason
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.domain.model.ClaimResult
import io.wolftown.kaiku.domain.model.ClaimStatus
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.testutil.FakeEnrollmentApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
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
import java.io.IOException
import java.time.Instant

@OptIn(ExperimentalCoroutinesApi::class)
class SetupFlowViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val token = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
    private val entry = SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.SCAN)
    private val enroll = FakeEnrollmentApi()
    private var decision: UvDecision = UvPolicy.choose(30, true, true)
    private var suitabilityChecks = 0
    private val suitability = object : DeviceSuitabilityChecker {
        override fun check(): UvDecision {
            suitabilityChecks++
            return decision
        }
    }
    private val deviceSignIn: DeviceSignIn = mockk()
    private val env = FakeSetupEnvironment()
    private val approval = FakeApprovalApi()
    private val probe = KeyDeviceProbe(approval) { SigFixtures.DEVICE_ID }

    /**
     * Runs inline but marks that key work went through the key dispatcher (never Main). With
     * [holdKeyWork] it keeps the work until [releaseKeyWork], like a key generation that takes seconds.
     */
    private var onKeyDispatcher = false
    private var holdKeyWork = false
    private val heldKeyWork = ArrayDeque<Runnable>()
    private val keyDispatcher = object : kotlinx.coroutines.CoroutineDispatcher() {
        override fun dispatch(context: kotlin.coroutines.CoroutineContext, block: Runnable) {
            if (holdKeyWork) {
                heldKeyWork.addLast(block)
                return
            }
            onKeyDispatcher = true
            try { block.run() } finally { onKeyDispatcher = false }
        }
    }

    /** Runs the key work held so far (work it queues meanwhile is held again). */
    private fun releaseKeyWork() {
        repeat(heldKeyWork.size) {
            val b = heldKeyWork.removeFirst()
            onKeyDispatcher = true
            try { b.run() } finally { onKeyDispatcher = false }
        }
    }

    /** The one setup of the phone [f]; every screen of a test shares it, as in the app. */
    private fun owner(f: KeyDeviceFixture): SetupOwner {
        env.hasRecord = { f.repo.record.value != null }
        return setupOwner(enroll.api, f.repo, f.signer, deviceSignIn, probe, env, suitability, keyDispatcher)
    }

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun TestScope.vm(): Pair<SetupFlowViewModel, KeyDeviceFixture> {
        // A fresh phone: no key device, no key, signed out.
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.keyStore.delete()
        f.authState.setLoggedOut()
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        val v = SetupFlowViewModel(owner(f))
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
        val v = SetupFlowViewModel(owner(f))
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
        v.confirmServer()
        // The setup goes on with that server, without making it the app's server address.
        assertNull(env.stored)
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
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller", "chat.example.com"), v.ui.value)
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
        coVerify { deviceSignIn.signIn(activity, io.wolftown.kaiku.data.auth.SignInPurpose.AFTER_SETUP) }
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

    @Test fun `already claimed never deletes the key - the server is asked what it holds`() = runTest {
        // "Already claimed" can be this phone's own claim that was stored when its answer got lost.
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        enroll.popAnswers += FakeEnrollmentApi.GIVEN // the challenge for the subject question
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED") // the claim check: still claimed
        approval.failWith = IOException("offline")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertTrue(f.keyStore.hasKey())
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(1, enroll.calls.count { it == "claim" })
    }

    /**
     * "Not valid" and "no key device" are only "not known": the key stays until the claim's time is
     * over by the server's and the phone's measure (or the claim check, under its lock, says more).
     */
    @Test fun `a link someone else used keeps the key while its server only does not know it`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = apiError(404, "ENROLLMENT_TOKEN_INVALID")
        enroll.popAnswers += FakeEnrollmentApi.GIVEN
        enroll.popAnswers += apiError(404, "ENROLLMENT_TOKEN_INVALID") // asked again under the enrolment's lock
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertTrue(v.ui.value is SetupUi.Ended)
        assertTrue(f.keyStore.hasKey())
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(1, enroll.calls.count { it == "claim" })
    }

    @Test fun `register binds this phone with the ticket and no code`() = runTest {
        env.pinned = null
        enroll.claimResult = ClaimResult("55555555-5555-4555-8555-555555555555", "done", authenticatorState = "active")
        val (v, _) = vm()
        v.start(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.TICKET, ticket = "ticket-secret"))
        // A ticket comes from this app's own registration: no server question.
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller", "chat.example.com"), v.ui.value)
        v.confirmSubject(activity)
        assertEquals("ticket-secret", enroll.lastTicket)
        assertFalse(enroll.calls.contains("status"))
        assertEquals(SetupUi.SigningIn, v.ui.value)
    }

    @Test fun `an active self-registration survives a restart before its sign-in`() = runTest {
        env.pinned = null
        enroll.claimResult = ClaimResult("55555555-5555-4555-8555-555555555555", "done", authenticatorState = "active")
        val (v, f) = vm()
        v.start(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.TICKET, ticket = "ticket-secret"))
        v.confirmSubject(activity)
        // C26: the self-claim answer has no claim secret; the key is active on the server.
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
        assertNull(f.repo.local()!!.claimSecret)
        // Without a session the app opens at "Confirm it's you" (device-key sign-in), never "Setup cancelled".
        assertEquals(KeyDeviceState.Lapsed(FakeEnrollmentApi.SUBJECT_ID), f.repo.current())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `a claim whose answer is lost keeps the key and the setup record`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = IOException("timeout")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertTrue(f.keyStore.hasKey())
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals("55555555-5555-4555-8555-555555555555", f.repo.local()!!.enrollmentId)
        assertEquals(KeyDeviceState.None, f.repo.current())
    }

    @Test fun `retry after a lost self-claim answer finds the active key and signs in`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = IOException("timeout")
        val (v, f) = vm()
        v.start(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.TICKET, ticket = "ticket-secret"))
        v.confirmSubject(activity)
        // The server had activated the key before the answer was lost: the enrolment is claimed,
        // and the key device is there.
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        v.retry()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
        assertEquals(1, enroll.calls.count { it == "claim" })
        assertEquals(listOf("deviceLoginChallenge:${FakeEnrollmentApi.SUBJECT_ID}:${SigFixtures.DEVICE_ID}"), approval.calls)
    }

    @Test fun `try again after a claim that never landed starts the same link again, never the same claim`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = IOException("timeout")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        val first = enroll.lastClaim!!
        val firstKey = f.keyStore.keyId()
        enroll.claimFailure = null
        // The enrolment is still open: the server hands out a new challenge, which voids the old claim.
        v.retry()
        assertEquals(1, enroll.calls.count { it == "claim" })
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller", "chat.example.com"), v.ui.value)
        assertNull(f.repo.local())
        assertTrue(f.keyStore.hasKey())
        assertTrue(f.keyStore.keyId() != firstKey)
        assertEquals(listOf(token, token), enroll.openTokens)
        // The person confirms once more: a new claim, signed by the new key.
        v.confirmSubject(activity)
        assertEquals(2, enroll.calls.count { it == "claim" })
        assertTrue(enroll.lastClaim!!.signature != first.signature)
        assertEquals(SigFixtures.b64u(f.keyStore.publicKeySpki()!!), enroll.lastClaim!!.publicKey)
        assertEquals(2, f.verifier.prompts.size)
        assertEquals("123 456", (v.ui.value as SetupUi.ShowCode).code)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
    }

    @Test fun `retry after a lost claim answer that someone else used ends without the key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = IOException("timeout")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        // The enrolment is claimed (by this phone's lost claim, or by someone else) and the server,
        // asked after that, lists no key device for this phone. This phone never got the claim's
        // answer, so no code of it can be confirmed: the key is worthless and a new code is needed.
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        v.retry()
        assertEquals(SetupUi.Ended(R.string.setup_already_used, canRetry = false), v.ui.value)
        assertFalse(f.keyStore.hasKey())
        assertNull(f.repo.local())
        assertEquals(1, enroll.calls.count { it == "claim" })
    }

    /** A screen lock set up or removed meanwhile, or a check that failed once, must not stick to the scanner. */
    @Test fun `the scanner checks what the phone can do anew whenever it is shown`() = runTest {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        val owner = owner(f)
        val noLock = UvDecision.Unsuitable(io.wolftown.kaiku.approval.UnsuitableReason.NO_SCREEN_LOCK)
        val fine = UvPolicy.choose(30, true, true)
        decision = noLock
        val first = SetupFlowViewModel(owner)
        first.scannerShown()
        assertEquals(noLock, first.scanReady.value)
        assertEquals(1, suitabilityChecks)
        // Shown again within the same visit (the screen is in front again, or a setup on it ended).
        decision = fine
        first.scannerShown()
        assertEquals(2, suitabilityChecks)
        assertEquals(fine, first.scanReady.value)
        // Meanwhile the earlier result stays on the screen, and one check runs at a time.
        decision = noLock
        holdKeyWork = true
        first.scannerShown()
        first.scannerShown()
        assertEquals(fine, first.scanReady.value)
        releaseKeyWork()
        assertEquals(3, suitabilityChecks)
        assertEquals(noLock, first.scanReady.value)
        first.onCleared()
        // A later visit: what the earlier visit found is not shown while the new result is found out.
        decision = fine
        val second = SetupFlowViewModel(owner)
        second.scannerShown()
        assertNull(second.scanReady.value)
        releaseKeyWork()
        assertEquals(4, suitabilityChecks)
        assertEquals(fine, second.scanReady.value)
    }

    /** The code on the screen and the link in the mail are one and the same setup code. */
    @Test fun `the same code scanned and then opened as a link is the same setup`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        v.start(entry)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        val key = f.keyStore.keyId()
        val asLink = SetupEntry(SigFixtures.ORIGIN + "/", token, SetupEntry.Source.LINK)
        // Opened while the scanned setup is held: not a second setup, and not "another link" either.
        v.start(asLink)
        assertFalse(v.linkIgnored.value)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        assertEquals(key, f.keyStore.keyId())
        assertEquals(listOf(token), enroll.openTokens)
        // A code for another setup is still another link.
        v.start(SetupEntry(SigFixtures.ORIGIN, "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8", SetupEntry.Source.LINK))
        assertTrue(v.linkIgnored.value)
    }

    @Test fun `a restored setup screen with a stored setup hands over to the waiting screen`() = runTest {
        val (v, f) = vm()
        f.keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        f.repo.saveClaimed(LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", "claim-secret", "Oma"))
        // Process death: the route came back, the setup code (memory only) did not.
        v.resumeWithoutEntry()
        assertEquals(SetupUi.Resume, v.ui.value)
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `a restored setup screen before any claim asks to scan the code again`() = runTest {
        val (v, _) = vm()
        v.resumeWithoutEntry()
        assertEquals(SetupUi.Interrupted, v.ui.value)
        assertTrue(enroll.calls.isEmpty())
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

    @Test fun `an unknown enrolment during the code wait keeps the key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statusFailures += apiError(404, "ENROLLMENT_TOKEN_INVALID")
        // The server knows no key device for this phone either.
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        // "Not known" is only that: the key stays (without the server's time nothing is counted).
        assertTrue(f.keyStore.hasKey())
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertTrue(v.ui.value is SetupUi.Ended)
    }

    /**
     * Fable v1 final check, minor 4: the status route also answers 404 once the
     * finished enrolment was swept (30 days after it ended); the key device decides.
     */
    @Test fun `a swept enrolment of an active key keeps the key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statusFailures += apiError(404, "ENROLLMENT_TOKEN_INVALID")
        approval.nextChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertTrue(f.keyStore.hasKey())
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
    }

    @Test fun `a gone enrolment while the server is unreachable keeps the key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statusFailures += apiError(404, "ENROLLMENT_TOKEN_INVALID")
        approval.failWith = IOException("offline")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertTrue(f.keyStore.hasKey())
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
    }

    @Test fun `a status answer that is not the server's own keeps the code on screen and the key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        // 410, a 404 without the server's code, an ending state under another status: none decides.
        enroll.statusFailures += apiError(410, "ENROLLMENT_FAILED")
        enroll.statusFailures += apiError(404, "NOT_FOUND")
        enroll.statusFailures += io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.Accepted, "status failed")
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        advanceTimeBy(20_000)
        runCurrent()
        assertTrue(f.keyStore.hasKey())
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals("123 456", (v.ui.value as SetupUi.ShowCode).code)
        assertTrue(approval.calls.isEmpty())
        assertEquals(4, enroll.calls.count { it == "status" })
    }

    @Test fun `a new code after an ended setup starts again`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, _) = vm()
        v.start(entry.copy(origin = "https://evil.example.com"))
        assertTrue(v.ui.value is SetupUi.Ended)
        v.reset()
        assertFalse(v.started)
        v.start(entry)
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller", "chat.example.com"), v.ui.value)
    }

    @Test fun `key work runs off the main thread`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        var generatedOffMain = false
        f.keyStore.onGenerate = { generatedOffMain = onKeyDispatcher }
        v.start(entry)
        assertTrue(generatedOffMain)
    }

    @Test fun `the status long-poll waits less than the client's read timeout`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, _) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertEquals(io.wolftown.kaiku.data.api.LongPoll.WAIT_SECONDS, enroll.lastStatusWait)
        assertTrue(io.wolftown.kaiku.data.api.LongPoll.WAIT_SECONDS < io.wolftown.kaiku.data.api.LongPoll.READ_TIMEOUT_SECONDS)
    }

    @Test fun `try again after an unsure answer for a waiting claim keeps the key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        // Done, pending, but no ready time: the probe is asked, and it cannot be reached.
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "pending")
        approval.failWith = IOException("offline")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        val spki = f.keyStore.publicKeySpki()!!
        val generated = f.keyStore.generatedChallenges.size

        v.retry()
        // The waiting screen asks the server; the key the server enrolled is never replaced.
        assertEquals(SetupUi.Resume, v.ui.value)
        assertTrue(f.keyStore.publicKeySpki()!!.contentEquals(spki))
        assertEquals(generated, f.keyStore.generatedChallenges.size)
    }

    @Test fun `a setup code scanned while a claim without its link is unresolved goes to the waiting screen`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = IOException("timeout")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        val generated = f.keyStore.generatedChallenges.size
        v.onCleared()
        // After a restart the claim's link is gone (it is never stored); the person scans again.
        val restarted = SetupFlowViewModel(owner(f))
        restarted.start(entry)
        assertEquals(SetupUi.Resume, restarted.ui.value)
        assertTrue(f.keyStore.hasKey())
        assertEquals(generated, f.keyStore.generatedChallenges.size)
        assertEquals(1, enroll.popCalls)
    }

    @Test fun `the same code scanned again while its claim is unresolved asks the enrolment first and makes no key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimFailure = IOException("timeout")
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        val key = f.keyStore.keyId()
        val generated = f.keyStore.generatedChallenges.size
        v.reset() // back to the scanner
        // The enrolment is claimed; what became of the key device cannot be asked right now.
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.failWith = IOException("offline")
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(2, enroll.popCalls)
        assertEquals(key, f.keyStore.keyId())
        assertEquals(generated, f.keyStore.generatedChallenges.size)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(1, enroll.calls.count { it == "claim" })
    }

    @Test fun `the subject screen and the prompt show the signed name and the server first`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.subject = io.wolftown.kaiku.domain.model.AccountRef(FakeEnrollmentApi.SUBJECT_ID, "oma", "Admin Bob")
        io.mockk.every { activity.getString(R.string.setup_uv_subtitle, any(), any()) } answers {
            "uv:" + it.invocation.args.drop(1).flatMap { a -> (a as? Array<*>)?.toList() ?: listOf(a) }.joinToString("|")
        }
        val (v, f) = vm()
        v.start(entry)
        assertEquals(SetupUi.ConfirmSubject("Admin Bob", "oma", "Familie Müller", "chat.example.com"), v.ui.value)
        v.confirmSubject(activity)
        assertEquals("uv:oma|chat.example.com", f.verifier.prompts.single().subtitle)
    }

    @Test fun `a PoP context that does not match this key, nonce or account is refused`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val tampers = listOf<(String) -> String>(
            { it.replace(Regex("\"attestation_challenge\":\"[^\"]*\""), "\"attestation_challenge\":\"${SigFixtures.b64u(ByteArray(32))}\"") },
            { it.replace(Regex("\"new_key_hash\":\"[^\"]*\""), "\"new_key_hash\":\"${SigFixtures.b64u(ByteArray(32))}\"") },
            { it.replace("\"subject_username\":\"oma\"", "\"subject_username\":\"mallory\"") },
        )
        for (t in tampers) {
            enroll.popContext = t // the server's hash is consistent: only the explicit checks catch it
            val (v, f) = vm()
            v.start(entry)
            assertEquals(SetupUi.Ended(R.string.setup_mismatch, canRetry = false), v.ui.value)
            assertTrue(f.verifier.prompts.isEmpty())
            assertFalse(f.keyStore.hasKey())
            assertFalse(enroll.calls.contains("claim"))
        }
    }

    @Test fun `a PoP prefix for another user id is refused`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.popPrefixUser = SigFixtures.USER_ID // not the subject the server named
        val (v, f) = vm()
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_mismatch, canRetry = false), v.ui.value)
        assertTrue(f.verifier.prompts.isEmpty())
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `cancel after confirming the server leaves the saved one as it was`() = runTest {
        env.stored = "https://typed.example.net"
        val (v, f) = vm()
        v.start(entry.copy(origin = "https://other.example.org"))
        v.confirmServer()
        assertEquals("https://typed.example.net", env.stored)
        v.cancel()
        assertEquals("https://typed.example.net", env.stored)
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `the same code again changes nothing, a newer one waits until the setup is stopped`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, _) = vm()
        v.start(entry)
        val opens = enroll.calls.count { it == "open" }
        val shown = v.ui.value
        v.start(entry.copy()) // a recreated screen passes the same code
        assertEquals(opens, enroll.calls.count { it == "open" })
        assertEquals(shown, v.ui.value)
        assertFalse(v.linkIgnored.value)
        val newer = entry.copy(tokenB64u = "BAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8")
        v.start(newer) // a newer link while this setup runs: not used
        assertEquals(opens, enroll.calls.count { it == "open" })
        assertEquals(shown, v.ui.value)
        assertTrue(v.linkIgnored.value)
        v.cancel()
        v.start(newer) // opened again after "Stop setup": now it starts
        assertEquals(opens + 1, enroll.calls.count { it == "open" })
        assertEquals(listOf(token, newer.tokenB64u), enroll.openTokens)
    }

    @Test fun `the phone's suitability is known before the scanner opens`() = runTest {
        decision = UvDecision.Unsuitable(UnsuitableReason.NO_SCREEN_LOCK)
        val (v, _) = vm()
        assertNull(v.scanReady.value)
        v.checkBeforeScan()
        assertEquals(UvDecision.Unsuitable(UnsuitableReason.NO_SCREEN_LOCK), v.scanReady.value)
        assertTrue(enroll.calls.isEmpty())
        decision = UvPolicy.choose(30, true, true)
        v.checkBeforeScan()
        assertTrue(v.scanReady.value is UvDecision.Use)
    }

    @Test fun `a second setup screen closing leaves the setup the first one shows`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.keyStore.delete()
        f.authState.setLoggedOut()
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        val one = owner(f)
        val below = SetupFlowViewModel(one)
        val onTop = SetupFlowViewModel(one)
        below.start(entry) // generates the key, now at the subject question
        val key = f.keyStore.keyId()
        onTop.onCleared() // the screen on top goes away
        assertEquals(key, f.keyStore.keyId())
        assertTrue(below.ui.value is SetupUi.ConfirmSubject)
        below.onCleared() // the last screen goes: the unclaimed key goes with it
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `a phone that already is the account's key is told so and offered to sign in`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.popFailure = apiError(400, "ENROLLMENT_DEVICE_IN_USE")
        val (v, f) = vm()
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_device_in_use, canRetry = false, offerSignIn = true), v.ui.value)
        assertFalse(f.keyStore.hasKey()) // the key made for this attempt goes; nothing was revoked
        assertFalse(enroll.calls.contains("claim"))
    }

    @Test fun `an expired ticket or request says so`() {
        assertEquals(SetupUi.Ended(R.string.setup_registration_expired, canRetry = false),
            setupErrorFor(apiError(404, "REGISTRATION_TICKET_INVALID")))
        assertEquals(SetupUi.Ended(R.string.setup_request_expired, canRetry = false),
            setupErrorFor(apiError(409, "ACCOUNT_REQUEST_NOT_READY", io.wolftown.kaiku.data.api.ApiErrorFields(state = "expired"))))
        assertEquals(SetupUi.Ended(R.string.setup_request_not_ready, canRetry = false),
            setupErrorFor(apiError(409, "ACCOUNT_REQUEST_NOT_READY", io.wolftown.kaiku.data.api.ApiErrorFields(state = "waiting"))))
    }

    @Test fun `stopping while the code is shown keeps the claim and says what happens`() = runTest {
        // Leaving at the code is explicit; a code typed in anyway still finishes at the next start.
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertTrue(v.ui.value is SetupUi.ShowCode)
        v.cancel()
        assertEquals(SetupUi.Ended(R.string.setup_stopped_after_code, canRetry = true), v.ui.value)
        assertTrue(f.keyStore.hasKey())
        assertEquals(io.wolftown.kaiku.data.keydevice.KeyDevicePhase.CLAIMED, f.repo.phase())
    }

    @Test fun `stopping before the claim still says nothing was changed`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        v.start(entry)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        v.cancel()
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
        assertNull(f.repo.local())
    }

    @Test fun `back asks at the code and at a question and waits while work runs`() {
        assertEquals(SetupBack.ASK, setupBackAction(SetupUi.ShowCode("123 456", null)))
        assertEquals(SetupBack.ASK, setupBackAction(SetupUi.WaitingForCode))
        assertEquals(SetupBack.ASK, setupBackAction(SetupUi.ConfirmServer("K", "chat.example.com")))
        assertEquals(SetupBack.ASK, setupBackAction(SetupUi.ConfirmSubject("A", "a", "K")))
        assertEquals(SetupBack.STAY, setupBackAction(SetupUi.Claiming))
        assertEquals(SetupBack.ASK, setupBackAction(SetupUi.ScreenLockNeeded(io.wolftown.kaiku.approval.UnsuitableReason.NO_SCREEN_LOCK)))
        assertEquals(SetupBack.CANCEL, setupBackAction(SetupUi.Opening))
        assertEquals(SetupBack.STAY, setupBackAction(SetupUi.SigningIn))
        assertEquals(SetupBack.STAY, setupBackAction(SetupUi.Done))
        assertEquals(SetupBack.LEAVE, setupBackAction(SetupUi.SignInPaused))
        assertEquals(SetupBack.LEAVE, setupBackAction(SetupUi.Ended(R.string.setup_stopped, canRetry = false)))
        assertEquals(SetupBack.LEAVE, setupBackAction(SetupUi.Interrupted))
    }

    private fun TestScope.lapsedKeyVm(): Pair<SetupFlowViewModel, KeyDeviceFixture> {
        val f = KeyDeviceFixture(backgroundScope)
        // The pin comes from the key (signed out): forgetting the key clears it, as on a phone.
        env.pinned = null
        env.pinFrom = { f.repo.pinnedOrigin() }
        f.authState.setLoggedOut() // a key without its session: "Confirm it's you"
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        val v = SetupFlowViewModel(owner(f))
        return v to f
    }

    @Test fun `a setup link on a phone whose key the admin removed forgets the key and goes on`() = runTest {
        // The server no longer knows the key, so "already your key" would be wrong.
        approval.failWith = apiError(403, "KEY_DEVICE_REVOKED")
        val (v, f) = lapsedKeyVm()
        v.start(entry)
        // The pin went with the key: the now unconfigured app asks about the server first.
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "chat.example.com"), v.ui.value)
        assertEquals(SigFixtures.ORIGIN, approval.deviceLoginOrigin)
        assertNull(f.repo.local())
    }

    @Test fun `a setup link on a lapsed key the server still knows is refused`() = runTest {
        approval.nextChallenge = SigFixtures.challengeFor(io.wolftown.kaiku.approval.SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)
        val (v, f) = lapsedKeyVm()
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_already_key, canRetry = false), v.ui.value)
        assertTrue(f.repo.local() != null)
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `a setup link on a lapsed key while offline is refused and keeps the key`() = runTest {
        approval.failWith = java.io.IOException("offline")
        val (v, f) = lapsedKeyVm()
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_already_key, canRetry = false), v.ui.value)
        assertTrue(f.repo.local() != null)
    }

    // ---- One link, one server: a later link or Back never redirects or disturbs a running step ----

    private val token2 = "BAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
    private val otherServer = "https://elsewhere.example.org"

    @Test fun `a link for another server while the old key is checked sends nothing there`() = runTest {
        approval.failWith = apiError(403, "KEY_DEVICE_REVOKED")
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { approval.challengeGate = it }
        val (v, f) = lapsedKeyVm()
        v.start(entry) // the old key's server is asked; the answer is slow
        v.start(SetupEntry(otherServer, token2, SetupEntry.Source.LINK))
        assertTrue(v.linkIgnored.value)
        gate.complete(Unit)
        runCurrent()
        // The first link goes on with its own server and code; the other server got nothing.
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "chat.example.com"), v.ui.value)
        assertEquals(listOf<String?>(SigFixtures.ORIGIN), enroll.origins)
        assertEquals(listOf(token), enroll.openTokens)
        assertNull(f.repo.local())
    }

    @Test fun `a second link for the same server while the old key is checked is not used`() = runTest {
        approval.failWith = apiError(403, "KEY_DEVICE_REVOKED")
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { approval.challengeGate = it }
        val (v, _) = lapsedKeyVm()
        v.start(entry)
        v.start(entry.copy(tokenB64u = token2))
        assertTrue(v.linkIgnored.value)
        gate.complete(Unit)
        runCurrent()
        assertEquals(listOf(token), enroll.openTokens)
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "chat.example.com"), v.ui.value)
    }

    @Test fun `a link for another server while the server is opened changes nothing`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { enroll.openGate = it }
        val (v, _) = vm()
        v.start(entry)
        v.start(SetupEntry(otherServer, token2, SetupEntry.Source.LINK))
        gate.complete(Unit)
        runCurrent()
        assertEquals(SetupUi.ConfirmSubject("Oma", "oma", "Familie Müller", "chat.example.com"), v.ui.value)
        assertTrue(enroll.origins.all { it == SigFixtures.ORIGIN })
        assertTrue(v.linkIgnored.value)
    }

    @Test fun `cancel while the server is opened stops the step for good`() = runTest {
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { enroll.openGate = it }
        val (v, _) = vm()
        v.start(entry)
        v.cancel()
        gate.complete(Unit)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
        assertFalse(enroll.calls.contains("popChallenge"))
    }

    @Test fun `back while the signed claim is on its way waits for it`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { enroll.claimGate = it }
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals(SetupBack.STAY, setupBackAction(v.ui.value))
        v.cancel() // also a stray Cancel
        v.start(entry.copy(tokenB64u = token2)) // and a new link
        gate.complete(Unit)
        runCurrent()
        assertEquals("123 456", (v.ui.value as SetupUi.ShowCode).code)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals(1, enroll.calls.count { it == "claim" })
    }

    @Test fun `the old key is kept when it changed while the server was asked`() = runTest {
        approval.failWith = apiError(403, "KEY_DEVICE_REVOKED")
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { approval.challengeGate = it }
        val (v, f) = lapsedKeyVm()
        v.start(entry)
        f.authState.setLoggedIn(SigFixtures.USER_ID) // signed in meanwhile: no longer the lapsed key it asked about
        runCurrent()
        gate.complete(Unit)
        runCurrent()
        assertTrue(f.repo.local() != null)
        assertEquals(SetupUi.Ended(R.string.setup_already_key, canRetry = false), v.ui.value)
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `back while the old key is checked says nothing was changed`() = runTest {
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { approval.challengeGate = it }
        val (v, f) = lapsedKeyVm()
        v.start(entry)
        v.cancel()
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
        gate.complete(Unit)
        runCurrent()
        assertTrue(f.repo.local() != null)
    }

    @Test fun `after stopping at the code the phone can keep waiting`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, _) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        v.cancel()
        assertEquals(SetupUi.Ended(R.string.setup_stopped_after_code, canRetry = true), v.ui.value)
        v.retry()
        assertEquals(SetupUi.Resume, v.ui.value)
    }

    @Test fun `a link on a phone whose session just ended is checked again`() = runTest {
        // Signed in when the link opened: "already your key". The session then turns out dead
        // (the admin removed this phone): the link is looked at again and the old key asked about.
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        val f = KeyDeviceFixture(backgroundScope)
        env.pinFrom = { f.repo.pinnedOrigin() }
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        val v = SetupFlowViewModel(owner(f))
        v.start(entry)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_already_key, canRetry = false), v.ui.value)
        f.authState.setLoggedOut()
        runCurrent()
        assertNull(f.repo.local())
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "chat.example.com"), v.ui.value)
    }

    @Test fun `a lapsed key is kept when its server answers with a removal code but another status`() = runTest {
        approval.failWith = apiError(503, "KEY_DEVICE_UNKNOWN")
        val (v, f) = lapsedKeyVm()
        v.start(entry)
        assertEquals(SetupUi.Ended(R.string.setup_already_key, canRetry = false), v.ui.value)
        assertTrue(f.repo.local() != null)
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `the final sign-in after a slow prompt offers a fresh try`() = runTest {
        // The challenge ran out while the prompt was open.
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        val (v, _) = vm()
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Expired
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        v.signIn(activity)
        assertEquals(SetupUi.SignInTimedOut, v.ui.value)
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        v.signIn(activity)
        assertEquals(SetupUi.Done, v.ui.value)
    }

    // ---- A setup in progress is never replaced by a link; the step in flight has one owner ----

    @Test fun `on a phone tied to no server a link for another server leaves the running setup alone`() = runTest {
        val (v, f) = vm() // no pin: nothing ties this phone to a server yet
        v.start(entry)
        v.confirmServer()
        val atQuestion = v.ui.value
        assertTrue(atQuestion is SetupUi.ConfirmSubject)
        val spki = f.keyStore.publicKeySpki()!!
        v.start(SetupEntry(otherServer, token2, SetupEntry.Source.LINK))
        assertTrue(v.linkIgnored.value)
        assertEquals(atQuestion, v.ui.value)
        assertTrue(enroll.origins.all { it == SigFixtures.ORIGIN })
        assertTrue(f.keyStore.publicKeySpki()!!.contentEquals(spki))
        // The setup never writes the app's server address.
        assertNull(env.stored)
    }

    @Test fun `on a phone tied to no server a link for another server at the server question changes nothing`() = runTest {
        val (v, _) = vm()
        v.start(entry)
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "chat.example.com"), v.ui.value)
        v.start(SetupEntry(otherServer, token2, SetupEntry.Source.LINK))
        assertTrue(v.linkIgnored.value)
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "chat.example.com"), v.ui.value)
        assertEquals(listOf(token), enroll.openTokens)
    }

    @Test fun `a second link for the same server never takes the code off the screen`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals("123 456", (v.ui.value as SetupUi.ShowCode).code)
        v.start(entry.copy(tokenB64u = token2))
        assertTrue(v.linkIgnored.value)
        assertEquals("123 456", (v.ui.value as SetupUi.ShowCode).code)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals(listOf(token), enroll.openTokens)
    }

    @Test fun `a link while the waiting screen is shown is not used and says so`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        f.keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        f.repo.saveClaimed(LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", "claim-secret", "Oma"))
        // A claim of this phone is stored: a link is never a new setup then, and the screen says so.
        v.start(entry)
        assertEquals(SetupUi.Resume, v.ui.value)
        assertTrue(v.linkIgnored.value)
        v.start(entry.copy(tokenB64u = token2))
        assertTrue(v.linkIgnored.value)
        assertEquals(SetupUi.Resume, v.ui.value)
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `the running link delivered again does not start it over`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { enroll.openGate = it }
        val (v, _) = vm()
        v.start(entry)
        v.start(entry.copy()) // the same link again while the server is opened
        gate.complete(Unit)
        runCurrent()
        assertEquals(1, enroll.openStarts)
        assertEquals(1, enroll.calls.count { it == "open" })
        assertFalse(v.linkIgnored.value)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
    }

    @Test fun `a step that starts the next step at once is still the one Cancel stops`() = runTest {
        // Signed in when the link opened ("already your key"); the session ends in the same moment
        // the step begins to wait for it, so the step starts the next one before it ever suspends.
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { approval.challengeGate = it }
        val f = KeyDeviceFixture(backgroundScope)
        env.pinFrom = { f.repo.pinnedOrigin() }
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        val v = SetupFlowViewModel(owner(f))
        backgroundScope.launch(kotlinx.coroutines.Dispatchers.Main) {
            v.ui.collect { if (it == SetupUi.Ended(R.string.setup_already_key, canRetry = false)) f.authState.setLoggedOut() }
        }
        v.start(entry)
        assertEquals(SetupUi.Opening, v.ui.value) // the old key's server is being asked
        v.cancel()
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
        gate.complete(Unit)
        runCurrent()
        // The stopped step neither forgot the key nor opened the setup.
        assertTrue(f.repo.local() != null)
        assertTrue(enroll.calls.isEmpty())
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
    }

    @Test fun `a key made while the setup was stopped is deleted`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (v, f) = vm()
        val generated = f.keyStore.generatedChallenges.size
        holdKeyWork = true
        v.start(entry)
        releaseKeyWork() // the suitability check; the key generation is held next
        assertEquals(1, heldKeyWork.size)
        v.cancel() // Back while the key is made
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
        holdKeyWork = false
        releaseKeyWork() // the generation finishes after the stop
        runCurrent()
        assertEquals(generated + 1, f.keyStore.generatedChallenges.size)
        assertFalse(f.keyStore.hasKey())
        assertFalse(enroll.calls.contains("popChallenge"))
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
    }

    @Test fun `a claim saved meanwhile by another setup screen is never forgotten`() = runTest {
        val (v, f) = vm()
        v.start(entry)
        assertTrue(v.ui.value is SetupUi.ConfirmServer)
        // Another setup screen claimed a code while this one waited at the server question.
        f.keyStore.generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
        val theirs = f.keyStore.publicKeySpki()!!
        val record = LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e9", "claim-secret", "Oma")
        f.repo.saveClaimed(record)
        val generated = f.keyStore.generatedChallenges.size
        v.confirmServer()
        assertEquals(SetupUi.Resume, v.ui.value)
        assertEquals(record, f.repo.local())
        assertTrue(f.keyStore.publicKeySpki()!!.contentEquals(theirs))
        assertEquals(generated, f.keyStore.generatedChallenges.size)
    }

    @Test fun `cancel while the PoP challenge is fetched deletes the new key and shows nothing more`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>().also { enroll.popGate = it }
        val (v, f) = vm()
        v.start(entry)
        assertTrue(f.keyStore.hasKey())
        v.cancel()
        gate.complete(Unit)
        runCurrent()
        assertFalse(f.keyStore.hasKey())
        assertEquals(SetupUi.Ended(R.string.setup_stopped, canRetry = false), v.ui.value)
    }

    @Test fun `a claim whose answer never comes ends as not sure after a while`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimGate = kotlinx.coroutines.CompletableDeferred() // never answered
        val (v, f) = vm()
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals(SetupUi.Claiming, v.ui.value)
        advanceTimeBy(SetupFlowViewModel.CLAIM_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `a final sign-in that never ends offers a fresh try after a while`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        val (v, _) = vm()
        coEvery { deviceSignIn.signIn(any(), any()) } coAnswers { kotlinx.coroutines.awaitCancellation() }
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        v.signIn(activity)
        assertEquals(SetupUi.SigningIn, v.ui.value)
        advanceTimeBy(SetupFlowViewModel.SIGN_IN_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.SignInTimedOut, v.ui.value)
    }

    @Test fun `back on the waiting screen waits while the final sign-in runs`() {
        assertEquals(SetupBack.STAY, pendingBackAction(SetupUi.SigningIn))
        assertEquals(SetupBack.STAY, pendingBackAction(SetupUi.Done))
        assertEquals(SetupBack.ASK, pendingBackAction(SetupUi.WaitingForCode))
        assertEquals(SetupBack.LEAVE, pendingBackAction(SetupUi.Opening))
        assertEquals(SetupBack.LEAVE, pendingBackAction(SetupUi.Pending(Instant.parse("2026-11-04T12:00:00Z"))))
        assertEquals(SetupBack.LEAVE, pendingBackAction(SetupUi.Ended(R.string.setup_cancelled, canRetry = false)))
    }
}
