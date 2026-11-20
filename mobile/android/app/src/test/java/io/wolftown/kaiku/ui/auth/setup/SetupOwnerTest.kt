package io.wolftown.kaiku.ui.auth.setup

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.keydevice.SetupDiscard
import io.wolftown.kaiku.domain.model.ClaimResult
import io.wolftown.kaiku.domain.model.ClaimStatus
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.FakeEnrollmentApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import io.wolftown.kaiku.testutil.markEnrolled
import io.wolftown.kaiku.testutil.saveClaimed
import io.wolftown.kaiku.testutil.seed
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.coroutines.withContext
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.IOException
import java.time.Instant

/**
 * One setup per app: the screens are views of one owner, links are noted while it holds a setup,
 * a claim is settled only by the listed answers, and the server is adopted only under the same
 * check as a link.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class SetupOwnerTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val token = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
    private val token2 = "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8"
    private val entry = SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.SCAN)
    private val link = SetupEntry(SigFixtures.ORIGIN, token2, SetupEntry.Source.LINK)
    private val enroll = FakeEnrollmentApi()
    private val deviceSignIn: DeviceSignIn = mockk()
    private val env = FakeSetupEnvironment()
    private val approval = FakeApprovalApi()
    /** The install id the app has now. */
    private var appInstallId = SigFixtures.DEVICE_ID
    private val probe = KeyDeviceProbe(approval) { appInstallId }
    /** The phone's own clock. */
    private var now: Instant = Instant.parse("2026-11-20T09:00:00Z")
    private val use = UvPolicy.choose(30, true, true) as UvDecision.Use
    private val activeChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    /** A fresh phone (no key device, no key, signed out) and its one setup. */
    private fun TestScope.phone(): Pair<SetupOwner, KeyDeviceFixture> {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.keyStore.delete()
        f.authState.setLoggedOut()
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        env.hasRecord = { f.repo.record.value != null }
        return owner(f) to f
    }

    /** A setup owner on the phone [f]: the app as it runs now, or (a second one) after a restart. */
    private fun owner(f: KeyDeviceFixture): SetupOwner =
        setupOwner(enroll.api, f.repo, f.signer, deviceSignIn, probe, env, clock = { now }, installId = { appInstallId })

    private fun noKeyDeviceAt(t: Instant?) = apiError(404, "KEY_DEVICE_UNKNOWN", ApiErrorFields(serverTime = t))

    // ── One setup per app ─────────────────────────────────────────────────────────────────────

    @Test fun `two screens show one setup`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val scanner = SetupFlowViewModel(owner)
        val linkScreen = SetupFlowViewModel(owner)
        val keysBefore = f.keyStore.generatedChallenges.size
        scanner.start(entry)
        assertTrue(scanner.ui.value is SetupUi.ConfirmSubject)
        assertEquals(scanner.ui.value, linkScreen.ui.value)
        val key = f.keyStore.keyId()
        // Whatever the second screen is handed, there is no second setup to start.
        linkScreen.start(link)
        assertTrue(linkScreen.linkIgnored.value && scanner.linkIgnored.value)
        assertEquals(key, f.keyStore.keyId())
        assertEquals(listOf(token), enroll.openTokens)
        assertEquals(keysBefore + 1, f.keyStore.generatedChallenges.size)
    }

    /**
     * A code is scanned and its question is open; a setup link is opened; later the person answers
     * the first question. There is one setup: the link is not used, nothing of it exists, and the
     * scanned setup finishes with its own key.
     */
    @Test fun `a link opened while the scanned setup shows its question starts nothing`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val scanner = SetupFlowViewModel(owner)
        scanner.start(entry)
        val key = f.keyStore.keyId()
        // The link arrives at the activity: not navigated, only noted.
        assertFalse(owner.offer(link))
        assertTrue(scanner.linkIgnored.value)
        // Even delivered to a setup screen, it is the same setup.
        val onTop = SetupFlowViewModel(owner)
        onTop.start(link)
        onTop.onCleared()
        assertEquals(key, f.keyStore.keyId())
        assertNull(f.repo.local())
        // The person answers the scanned setup's question: its own key signs its own claim.
        scanner.confirmSubject(activity)
        assertEquals(1, enroll.calls.count { it == "claim" })
        assertEquals(SigFixtures.b64u(f.keyStore.publicKeySpki()!!), enroll.lastClaim!!.publicKey)
        assertEquals(token, enroll.lastClaim!!.setupToken)
        assertEquals(key, f.keyStore.keyId())
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals(key, f.repo.record.value!!.keyId)
        assertEquals(listOf(key), f.verifier.keyIds)
    }

    @Test fun `a link opened while nothing is held goes to the setup screen and changes nothing yet`() = runTest {
        val (owner, _) = phone()
        assertTrue(owner.offer(link))
        assertFalse(owner.linkIgnored.value)
        assertFalse(owner.held())
        assertTrue(enroll.calls.isEmpty())
    }

    @Test fun `a link opened over the waiting screen is noted there and not used`() = runTest {
        val (owner, f) = phone()
        f.keyStore.generate(ByteArray(32), use)
        f.repo.saveClaimed(LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", "claim-secret", "Oma"))
        val waiting = SetupPendingViewModel(owner) // the app started at the waiting screen
        assertEquals(SetupUi.WaitingForCode, waiting.ui.value)
        assertTrue(owner.held())
        assertFalse(owner.offer(link))
        assertTrue(waiting.linkIgnored.value)
        assertFalse(enroll.calls.contains("open"))
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
    }

    /** The guide's remedy: once the waiting screen says the code ran out, the link opened again works. */
    @Test fun `after the waiting screen reported the end, a link opened again starts`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        f.keyStore.generate(ByteArray(32), use)
        f.repo.saveClaimed(LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", "claim-secret", "Oma"))
        val setupScreen = SetupFlowViewModel(owner)
        setupScreen.start(entry) // a claim is stored: the waiting screen takes over inside the setup screen
        assertEquals(SetupUi.Resume, setupScreen.ui.value)
        val waiting = SetupPendingViewModel(owner)
        enroll.statuses += ClaimStatus(EnrollmentState.EXPIRED)
        enroll.statuses += ClaimStatus(EnrollmentState.EXPIRED)
        waiting.check()
        advanceTimeBy(SetupOwner.EXPIRED_RECHECK_MS + 1)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_expired, canRetry = false), waiting.ui.value)
        assertFalse(owner.held())
        assertNull(f.repo.local())
        // The link is opened again, without leaving the screen first.
        assertTrue(owner.offer(link))
        setupScreen.start(link)
        assertFalse(setupScreen.linkIgnored.value)
        assertEquals(listOf(token2), enroll.openTokens)
        assertTrue(setupScreen.ui.value is SetupUi.ConfirmSubject)
        assertFalse(setupScreen.waiting.value)
    }

    /**
     * The link waits in one place until the setup screen takes it. The screen's effect runs once
     * per value of that place: with the link, and again when taking it emptied the place.
     */
    @Test fun `one delivery of a link is one start`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, _) = phone()
        val screen = SetupFlowViewModel(owner)
        val pending = kotlinx.coroutines.flow.MutableStateFlow<SetupEntry?>(null)
        val starts = mutableListOf<SetupEntry>()
        val effect = backgroundScope.launch(Dispatchers.Main) {
            pending.collect { link ->
                deliverSetupLink(link, screen.started, { starts += it; screen.start(it) }, { pending.compareAndSet(it, null) }, screen::resumeWithoutEntry)
            }
        }
        // Restored without a link: it looks at what is stored, once.
        assertEquals(SetupUi.Interrupted, screen.ui.value)
        // A setup that fails at once (the link's server cannot be reached) ...
        enroll.openGate = CompletableDeferred<Unit>().also { it.completeExceptionally(IOException("offline")) }
        pending.value = link
        runCurrent()
        // ... is started once, not once more by the emptied place.
        assertEquals(listOf(link), starts)
        assertEquals(1, enroll.openStarts)
        assertNull(pending.value)
        // The same link opened again is a new delivery.
        pending.value = link
        runCurrent()
        assertEquals(listOf(link, link), starts)
        assertEquals(2, enroll.openStarts)
        effect.cancel()
    }

    @Test fun `when the last screen closes an unclaimed key goes and the next screen starts clean`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val screen = SetupFlowViewModel(owner)
        screen.start(entry)
        assertTrue(f.keyStore.hasKey())
        screen.onCleared()
        assertFalse(f.keyStore.hasKey())
        assertFalse(owner.held())
        val next = SetupFlowViewModel(owner)
        assertFalse(next.started)
        assertEquals(SetupUi.Opening, next.ui.value)
    }

    @Test fun `when the last screen closes a stored claim and its key stay`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val screen = SetupFlowViewModel(owner)
        screen.start(entry)
        screen.confirmSubject(activity)
        val key = f.keyStore.keyId()
        screen.onCleared()
        assertEquals(key, f.keyStore.keyId())
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
    }

    @Test fun `the waiting screen takes the stored setup up again after a link's screen above it closed`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        f.keyStore.generate(ByteArray(32), use)
        f.repo.markEnrolled(LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null, "Oma"))
        val waiting = SetupPendingViewModel(owner)
        assertEquals(SetupUi.SigningIn, waiting.ui.value)
        // The final sign-in fails for good: the waiting screen says why and holds nothing any more.
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Banned
        waiting.signIn(activity)
        assertTrue(waiting.ui.value is SetupUi.Ended)
        assertFalse(owner.held())
        // A link is opened and looked at on a setup screen above ("already your key"), which then closes.
        assertTrue(owner.offer(link))
        approval.nextChallenge = activeChallenge
        val above = SetupFlowViewModel(owner)
        above.start(link)
        assertEquals(SetupUi.Ended(R.string.setup_already_key, canRetry = false), above.ui.value)
        above.onCleared()
        // The waiting screen is in front again and checks: it shows the stored setup, not a blank one.
        waiting.check()
        assertTrue(owner.waiting.value)
        assertEquals(SetupUi.SigningIn, waiting.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
    }

    @Test fun `a server adopted by one setup is not put back by a later one`() = runTest {
        val (owner, f) = phone()
        val first = SetupFlowViewModel(owner)
        env.stored = "https://typed.example.net" // a server the person had typed in before
        first.start(entry)
        first.confirmServer()
        assertEquals(SigFixtures.ORIGIN, env.stored)
        first.confirmSubject(activity) // claimed: the phone is tied to this server now
        first.onCleared()
        assertEquals(SigFixtures.ORIGIN, env.stored)
        // A later setup screen that ends without anything of its own leaves the server alone.
        env.pinned = SigFixtures.ORIGIN
        val later = SetupFlowViewModel(owner)
        later.start(entry.copy(origin = "https://evil.example.com"))
        later.onCleared()
        assertEquals(SigFixtures.ORIGIN, env.stored)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
    }

    @Test fun `a setup that adopted a server and is abandoned puts the earlier one back`() = runTest {
        val (owner, _) = phone()
        val v = SetupFlowViewModel(owner)
        env.stored = "https://typed.example.net"
        v.start(entry)
        v.confirmServer()
        assertEquals(SigFixtures.ORIGIN, env.stored)
        v.onCleared()
        assertEquals("https://typed.example.net", env.stored)
    }

    @Test fun `a store that cannot be read starts no setup and costs no key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        f.keyStore.generate(ByteArray(32), use)
        val key = f.keyStore.keyId()
        f.store.unreadable = true
        f.repo.reload()
        val screen = SetupFlowViewModel(owner)
        screen.start(entry)
        assertEquals(SetupUi.Ended(R.string.security_key_storage_unreadable, canRetry = false), screen.ui.value)
        assertTrue(enroll.calls.isEmpty())
        screen.onCleared()
        // The key in the alias is not an unclaimed left-over just because its record cannot be read.
        assertEquals(key, f.keyStore.keyId())
        assertTrue(f.keyStore.deleteAsked.isEmpty())
    }

    // ── The claim and its answers ─────────────────────────────────────────────────────────────

    private fun TestScope.atClaim(failure: Exception?): Triple<SetupFlowViewModel, KeyDeviceFixture, SetupOwner> {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.claimFailure = failure
        v.confirmSubject(activity)
        return Triple(v, f, owner)
    }

    @Test fun `a claim refused for good forgets its own key and record`() = runTest {
        for (x in listOf(
            apiError(400, "VALIDATION_ERROR"), apiError(400, "SIGNATURE_INVALID"), apiError(400, "DEVICE_UNSUITABLE"),
            apiError(404, "APPROVAL_NOT_FOUND"), apiError(410, "APPROVAL_EXPIRED"),
        )) {
            val (v, f, _) = atClaim(x)
            assertEquals(x.code, setupErrorFor(x), v.ui.value)
            assertFalse(x.code, f.keyStore.hasKey())
            assertNull(x.code, f.repo.local())
            assertEquals(1, enroll.popCalls)
            enroll.popCalls = 0
        }
    }

    @Test fun `any other answer to a claim keeps the key and the record`() = runTest {
        for (x in listOf<Exception>(
            apiError(400, "ENROLLMENT_DEVICE_IN_USE"), apiError(403, "APPROVAL_NOT_ALLOWED"), apiError(403, "ACCOUNT_BANNED"),
            apiError(429, "TOO_MANY_ATTEMPTS"), apiError(500, "INTERNAL_ERROR"), apiError(503, "SERVICE_UNAVAILABLE"),
            apiError(404, "NOT_FOUND"), apiError(400, "SOMETHING_NEW"), apiError(418, "SIGNATURE_INVALID"),
            apiError(500, "VALIDATION_ERROR"), apiError(401, "INVALID_TOKEN"), apiError(422, "VALIDATION_ERROR"),
            // A success that is not the contract's 201, a body nobody could read, no connection.
            io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.OK, "claim failed"),
            io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.BadRequest, "claim failed"),
            kotlinx.serialization.SerializationException("not json"), IOException("reset"),
        )) {
            val (v, f, _) = atClaim(x)
            val what = (x as? io.wolftown.kaiku.data.api.ApiException)?.let { "${it.status.value} ${it.code}" } ?: x.toString()
            assertTrue(what, v.ui.value is SetupUi.Ended && (v.ui.value as SetupUi.Ended).canRetry)
            assertTrue(what, f.keyStore.hasKey())
            assertEquals(what, KeyDevicePhase.CLAIMING, f.repo.phase())
            // Not checked by itself either: the person decides to try again.
            assertEquals(what, 1, enroll.popCalls)
            enroll.popCalls = 0
        }
    }

    @Test fun `an undecided claim answer is checked with the enrolment, never sent again`() = runTest {
        for (x in listOf(
            apiError(409, "ENROLLMENT_ALREADY_CLAIMED"), apiError(404, "ENROLLMENT_TOKEN_INVALID"),
            apiError(409, "APPROVAL_STATE"), apiError(409, "ENROLLMENT_STATE"),
        )) {
            enroll.popAnswers.clear()
            enroll.popAnswers += FakeEnrollmentApi.GIVEN
            enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
            approval.failWith = IOException("offline")
            val (v, f, _) = atClaim(x)
            assertEquals(x.code, 2, enroll.popCalls)
            assertEquals(x.code, 1, enroll.calls.count { it == "claim" })
            assertEquals(x.code, SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
            assertTrue(x.code, f.keyStore.hasKey())
            assertEquals(x.code, KeyDevicePhase.CLAIMING, f.repo.phase())
            enroll.popCalls = 0
            enroll.calls.clear()
        }
    }

    /**
     * The claim's answer does not come within its limit, but the server stores it (a claim of the
     * app's own registration makes the key device in the same step). The enrolment then says
     * "already claimed", and the key device, asked after that answer, is there: the setup goes on
     * with the same key, and the claim is never sent again.
     */
    @Test fun `a claim that times out and was stored is recovered, not replayed into a refusal`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimResult = ClaimResult("55555555-5555-4555-8555-555555555555", "done", authenticatorState = "active")
        enroll.claimGate = CompletableDeferred() // the answer never arrives
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.TICKET, ticket = "ticket-secret"))
        v.confirmSubject(activity)
        assertEquals(SetupUi.Claiming, v.ui.value)
        advanceTimeBy(SetupOwner.CLAIM_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        val key = f.keyStore.keyId()

        // "Try again": the enrolment is claimed, and the key device is listed.
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.nextChallenge = activeChallenge
        v.retry()
        assertTrue(owner.held())
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        assertEquals(0, enroll.calls.count { it == "claim" }) // the held request never completed; none was sent again
    }

    // ── A claim whose answer was lost ─────────────────────────────────────────────────────────

    /**
     * The signed claim left the phone and its answer never arrived (no code was shown). The screen
     * says the connection broke; the record says the claim's answer is not known.
     */
    private fun TestScope.lostClaim(): Triple<SetupFlowViewModel, KeyDeviceFixture, SetupOwner> {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimGate = CompletableDeferred()
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        advanceTimeBy(SetupOwner.CLAIM_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        return Triple(v, f, owner)
    }

    /** The server stored that claim (or another phone used the link): the enrolment is claimed, no key device came of it here. */
    private fun serverHoldsTheClaim() {
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN") // the server's time is not needed
    }

    @Test fun `a lost claim is given up at once when its enrolment is claimed and no key device came of it`() = runTest {
        val (v, f, owner) = lostClaim()
        serverHoldsTheClaim()
        v.retry()
        assertEquals(SetupUi.Ended(R.string.setup_already_used, canRetry = false), v.ui.value)
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
        assertEquals("clear", f.store.writes.last())
        // The claim was never sent again; the key device was asked once, after the enrolment.
        assertEquals(0, enroll.calls.count { it == "claim" })
        assertEquals(listOf("deviceLoginChallenge:${FakeEnrollmentApi.SUBJECT_ID}:${SigFixtures.DEVICE_ID}"), approval.calls)
        // Nothing is held any more: a fresh link is a setup of its own.
        assertFalse(owner.held())
        assertTrue(owner.offer(link))
        v.start(link)
        assertFalse(v.linkIgnored.value)
        assertEquals(listOf(token, token2), enroll.openTokens)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        assertTrue(f.keyStore.hasKey())
    }

    /** The claim itself is answered "already claimed" (a request sent twice by the network stack, or another phone was first). */
    @Test fun `a claim answered 'already claimed' with no key device is given up without a wait`() = runTest {
        enroll.popAnswers += FakeEnrollmentApi.GIVEN // the challenge for the subject question
        serverHoldsTheClaim()
        val (v, f, owner) = atClaim(apiError(409, "ENROLLMENT_ALREADY_CLAIMED"))
        assertEquals(SetupUi.Ended(R.string.setup_already_used, canRetry = false), v.ui.value)
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
        assertFalse(owner.held())
        assertEquals(1, enroll.calls.count { it == "claim" })
    }

    /** The admin sent a new link while the phone still says "the connection broke". Opening it settles the old claim and starts. */
    @Test fun `a fresh link opened over a lost claim settles that claim and then starts`() = runTest {
        val (v, f, owner) = lostClaim()
        val oldKey = f.keyStore.keyId()
        serverHoldsTheClaim()
        assertTrue(owner.offer(link))
        v.start(link)
        // The stored claim was asked about with its own link and key; then the new link started.
        assertEquals(listOf(token, token2), enroll.openTokens)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        assertFalse(v.linkIgnored.value)
        assertNull(f.repo.local())
        assertTrue(f.keyStore.hasKey())
        assertTrue(f.keyStore.keyId() != oldKey)
        assertTrue(owner.held())
        // It is a setup like any other: the person confirms, the code is shown.
        enroll.claimGate = null
        v.confirmSubject(activity)
        assertEquals(token2, enroll.lastClaim!!.setupToken)
        assertTrue(v.ui.value is SetupUi.ShowCode)
    }

    @Test fun `a fresh link opened while the server cannot be reached starts once 'Try again' settled the old claim`() = runTest {
        val (v, f, owner) = lostClaim()
        enroll.popAnswers += IOException("offline")
        assertTrue(owner.offer(link))
        v.start(link)
        // Not used yet, and the screen says so; the stored claim is untouched.
        assertTrue(v.linkIgnored.value)
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(listOf(token), enroll.openTokens)
        serverHoldsTheClaim()
        v.retry()
        assertEquals(listOf(token, token2), enroll.openTokens)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        assertFalse(v.linkIgnored.value)
    }

    @Test fun `the key device is asked only after the enrolment has answered`() = runTest {
        val (v, f, _) = lostClaim()
        serverHoldsTheClaim()
        val held = CompletableDeferred<Unit>().also { enroll.popGate = it }
        v.retry()
        runCurrent()
        // The enrolment has not answered yet: nothing was asked about the key device.
        assertTrue(approval.calls.isEmpty())
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        held.complete(Unit)
        runCurrent()
        assertEquals(1, approval.calls.size)
        assertNull(f.repo.local())
    }

    @Test fun `a lost claim that became a key device by itself is taken over`() = runTest {
        // Active: the final sign-in follows.
        val (v, f, _) = lostClaim()
        val key = f.keyStore.keyId()
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.nextChallenge = activeChallenge
        v.retry()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
    }

    @Test fun `a lost claim that became a waiting key device is taken over`() = runTest {
        val (v, f, _) = lostClaim()
        val key = f.keyStore.keyId()
        val readyAt = Instant.parse("2026-11-21T09:00:00Z")
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.failWith = apiError(409, "KEY_DEVICE_PENDING", ApiErrorFields(readyAt = readyAt.toString()))
        v.retry()
        assertEquals(SetupUi.Pending(readyAt), v.ui.value)
        assertEquals(KeyDevicePhase.PENDING, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
    }

    @Test fun `a lost claim is kept when the enrolment says anything else`() = runTest {
        val (v, f, _) = lostClaim()
        val key = f.keyStore.keyId()
        approval.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        for (x in listOf<Exception>(
            apiError(409, "ENROLLMENT_STATE"), apiError(400, "ENROLLMENT_DEVICE_IN_USE"), apiError(500, "INTERNAL_ERROR"),
            apiError(429, "TOO_MANY_ATTEMPTS"), apiError(400, "ENROLLMENT_ALREADY_CLAIMED"), apiError(503, "ENROLLMENT_ALREADY_CLAIMED"),
        )) {
            enroll.popAnswers += x
            v.retry()
            assertEquals(x.toString(), SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
            assertEquals(x.toString(), key, f.keyStore.keyId())
            assertEquals(x.toString(), KeyDevicePhase.CLAIMING, f.repo.phase())
        }
        assertEquals(0, enroll.calls.count { it == "claim" })
    }

    @Test fun `a lost claim is kept when the key device cannot be asked`() = runTest {
        val (v, f, _) = lostClaim()
        for (x in listOf<Exception>(IOException("offline"), apiError(500, "INTERNAL_ERROR"), apiError(503, "KEY_DEVICE_UNKNOWN"), apiError(403, "ACCOUNT_BANNED"))) {
            enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
            approval.failWith = x
            v.retry()
            assertEquals(x.toString(), SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
            assertTrue(x.toString(), f.keyStore.hasKey())
            assertEquals(x.toString(), KeyDevicePhase.CLAIMING, f.repo.phase())
        }
    }

    /** A code that was shown may still be typed in and confirmed: such a claim waits for its server's word. */
    @Test fun `a claim whose code was shown is not given up by a fresh link`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        assertTrue(v.ui.value is SetupUi.ShowCode)
        v.cancel()
        val key = f.keyStore.keyId()
        val asked = enroll.popCalls
        serverHoldsTheClaim()
        assertTrue(owner.offer(link))
        v.start(link)
        assertEquals(SetupUi.Resume, v.ui.value)
        assertTrue(v.linkIgnored.value)
        assertEquals(asked, enroll.popCalls)
        assertTrue(approval.calls.isEmpty())
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        assertEquals(listOf(token), enroll.openTokens)
    }

    /** After a restart the link is gone (it is never stored): only the key device can be asked, and that waits for its time. */
    @Test fun `a lost claim without its link keeps waiting for its time`() = runTest {
        val (first, f, _) = lostClaim()
        first.onCleared()
        val until = f.repo.record.value!!.claimVoidAfter!!
        val restarted = owner(f)
        val v = SetupFlowViewModel(restarted)
        val asked = enroll.popCalls
        approval.failWith = noKeyDeviceAt(until.minusSeconds(60))
        v.start(link)
        assertEquals(SetupUi.Resume, v.ui.value)
        assertTrue(v.linkIgnored.value)
        val waiting = SetupPendingViewModel(restarted)
        waiting.check()
        assertEquals(SetupUi.ClaimWaiting, waiting.ui.value)
        assertEquals(asked, enroll.popCalls)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
        assertEquals(listOf(token), enroll.openTokens)
        // The screens close: the wait ends with them.
        waiting.onCleared()
        v.onCleared()
    }

    /** The link of a lost claim stays in memory while that claim is stored, also when its screen has closed. */
    @Test fun `a lost claim is settled with its link after its screen closed, on the waiting screen too`() = runTest {
        val (first, f, owner) = lostClaim()
        first.onCleared()
        assertFalse(owner.held())
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        serverHoldsTheClaim()
        val waiting = SetupPendingViewModel(owner)
        waiting.check()
        assertEquals(SetupUi.Ended(R.string.setup_already_used, canRetry = false), waiting.ui.value)
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
        assertEquals(2, enroll.popCalls) // the challenge for the subject question, then the claim check
    }

    @Test fun `on the waiting screen a lost claim whose enrolment is still open ends, and the link opened again starts`() = runTest {
        val (first, f, owner) = lostClaim()
        first.onCleared()
        val waiting = SetupPendingViewModel(owner)
        // The enrolment hands out a new challenge: it is open, and the claim never reached it.
        waiting.check()
        assertEquals(SetupUi.Ended(R.string.setup_not_finished, canRetry = false), waiting.ui.value)
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
        assertFalse(owner.held())
        assertTrue(owner.offer(entry))
    }

    /**
     * The last screen closes while the claim is on its way: the request is given up, the record
     * written before it and its key stay, no code is shown anywhere, and the next look asks the server.
     */
    @Test fun `the last screen closing while the claim is on its way leaves the claim to be checked`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimGate = CompletableDeferred()
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        assertEquals(SetupUi.Claiming, v.ui.value)
        val key = f.keyStore.keyId()
        v.onCleared()
        assertFalse(owner.held())
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        // An answer that still arrives belongs to no screen: nothing is stored, nothing is shown.
        enroll.claimGate!!.complete(Unit)
        runCurrent()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(SetupUi.Opening, owner.ui.value)
        assertEquals(listOf("save:CLAIMING"), f.store.writes)
        // The waiting screen asks: the server did store the claim, and no code of it was ever seen.
        serverHoldsTheClaim()
        val waiting = SetupPendingViewModel(owner)
        waiting.check()
        assertEquals(SetupUi.Ended(R.string.setup_already_used, canRetry = false), waiting.ui.value)
        assertNull(f.repo.local())
    }

    /** The server is asked about a setup under the install id it was claimed with, whatever id the app has later. */
    @Test fun `a setup is asked about under the install id it was claimed with`() = runTest {
        val (first, f, _) = lostClaim()
        assertEquals(SigFixtures.DEVICE_ID, f.repo.record.value!!.installId)
        first.onCleared()
        // The app's own id was lost and made anew (it is not kept with the key record).
        appInstallId = "0a0a0a0a-0a0a-4a0a-8a0a-0a0a0a0a0a0a"
        val waiting = SetupPendingViewModel(owner(f))
        approval.failWith = noKeyDeviceAt(null)
        waiting.check()
        assertEquals(listOf("deviceLoginChallenge:${FakeEnrollmentApi.SUBJECT_ID}:${SigFixtures.DEVICE_ID}"), approval.calls)
        assertEquals(SigFixtures.DEVICE_ID, f.repo.record.value!!.installId)
    }

    @Test fun `a claim check answered with a challenge of another enrolment or key proves nothing`() = runTest {
        for (rewrite in listOf<(String) -> String>(
            { it.replace("55555555-5555-4555-8555-555555555555", "55555555-5555-4555-8555-000000000009") },
            { it.replace(Regex("\"new_key_hash\":\"[^\"]*\""), "\"new_key_hash\":\"SNXZ3WXZTk1gg0TCFd1P6gmdzP1Rwao9hsqXg-SsX10\"") },
        )) {
            val (v, f, _) = atClaim(IOException("reset"))
            val key = f.keyStore.keyId()
            enroll.popContext = rewrite
            v.retry()
            assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
            assertEquals(key, f.keyStore.keyId())
            assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
            enroll.popContext = { it }
        }
    }

    @Test fun `the claim records when its time is over from the challenge it signed`() = runTest {
        val (_, f, _) = atClaim(IOException("reset"))
        // The challenge of the fixture runs out at EXPIRES_AT; a code would live 5 minutes more, plus a margin.
        assertEquals(Instant.ofEpochSecond(SigFixtures.EXPIRES_AT).plusSeconds(600), f.repo.record.value!!.claimVoidAfter)
    }

    @Test fun `nothing is sent when the claim cannot be recorded first`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        f.store.failWrites = true
        v.confirmSubject(activity)
        assertFalse(enroll.calls.contains("claim"))
        assertEquals(SetupUi.Ended(R.string.setup_failed_generic, canRetry = true), v.ui.value)
        assertNull(f.repo.local())
    }

    @Test fun `a key that is no longer this setup's key signs no claim and writes no record`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        // The alias got another key behind the setup's back (it cannot through this app's own code).
        f.keyStore.delete()
        val other = f.keyStore.generate(ByteArray(32), use).keyId
        v.confirmSubject(activity)
        assertFalse(enroll.calls.contains("claim"))
        assertNull(f.repo.local())
        assertTrue(f.verifier.prompts.isEmpty())
        // Its own key is gone, so there is nothing of its own to delete; the other key stays.
        assertEquals(other, f.keyStore.keyId())
    }

    /**
     * A stored "answer not known" must mean that no code of that claim was ever on this screen.
     * So a code whose arrival could not be recorded is not shown; the claim is checked like any
     * other whose answer is not known.
     */
    @Test fun `a code is shown only once its arrival is recorded`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.claimGate = CompletableDeferred()
        v.confirmSubject(activity)
        f.store.failWrites = true // storage fails after the claim was recorded and sent
        enroll.claimGate!!.complete(Unit)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_failed_generic, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
        assertFalse(enroll.calls.contains("status"))
        // "Try again": the enrolment is claimed, this phone is no key device, and nobody saw the code.
        f.store.failWrites = false
        serverHoldsTheClaim()
        v.retry()
        assertEquals(SetupUi.Ended(R.string.setup_already_used, canRetry = false), v.ui.value)
        assertNull(f.repo.local())
        assertEquals(1, enroll.calls.count { it == "claim" })
    }

    // ── A setup whose server cannot be asked ──────────────────────────────────────────────────

    /** A claim waits for its code; the app was closed; the waiting screen of the next start. */
    private fun TestScope.waitingForCode(): Triple<SetupPendingViewModel, KeyDeviceFixture, SetupOwner> {
        env.pinned = SigFixtures.ORIGIN
        val (first, f) = phone()
        val v = SetupFlowViewModel(first)
        v.start(entry)
        v.confirmSubject(activity)
        assertTrue(v.ui.value is SetupUi.ShowCode)
        v.onCleared()
        val later = owner(f)
        return Triple(SetupPendingViewModel(later), f, later)
    }

    /** One question without an answer: the screen says so and keeps waiting. Nothing more, however old the setup. */
    @Test fun `one failed question says only that the server could not be asked`() = runTest {
        val (waiting, f, _) = waitingForCode()
        now = now.plusSeconds(30L * 24 * 3600)
        enroll.statusFailures += IOException("no route to host")
        waiting.check()
        assertEquals(SetupUi.WaitingForCode, waiting.ui.value)
        assertTrue(waiting.cannotAsk.value)
        assertFalse(waiting.lastResort.value)
        assertFalse(waiting.discardable.value)
        assertEquals(1, f.repo.record.value!!.unanswered!!.occasions)
        // A server fault and "too many requests" are no answer either; a refusal of the server is one.
        for (x in listOf(apiError(502, "BAD_GATEWAY"), apiError(429, "TOO_MANY_ATTEMPTS"))) {
            enroll.statusFailures += x
            waiting.check()
            assertTrue(x.code, waiting.cannotAsk.value)
        }
        enroll.statusFailures += apiError(403, "ACCOUNT_BANNED")
        waiting.check()
        assertFalse(waiting.cannotAsk.value)
        assertNull(f.repo.record.value!!.unanswered)
    }

    @Test fun `the last resort is named in words once the silence has lasted, and taken back by the next answer`() = runTest {
        val (waiting, f, _) = waitingForCode()
        val key = f.keyStore.keyId()
        // Three occasions over a day and more.
        for (hours in listOf(2L, 10, 27)) {
            assertFalse(waiting.lastResort.value)
            now = now.plusSeconds(hours * 3600)
            enroll.statusFailures += IOException("no route to host")
            waiting.check()
            assertTrue(waiting.cannotAsk.value)
        }
        assertTrue(waiting.lastResort.value)
        assertFalse(waiting.discardable.value)
        // The server is back: the claim still waits for its code.
        enroll.statuses += ClaimStatus(EnrollmentState.CLAIMED)
        waiting.check()
        runCurrent()
        assertFalse(waiting.cannotAsk.value)
        assertFalse(waiting.lastResort.value)
        assertNull(f.repo.record.value!!.unanswered)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
    }

    @Test fun `with the code on screen a failed question is noted, and the next answer takes the note back`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, _) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.statusFailures += IOException("offline")
        enroll.statuses += ClaimStatus(EnrollmentState.CLAIMED)
        v.confirmSubject(activity)
        assertTrue(v.ui.value is SetupUi.ShowCode)
        assertTrue(v.cannotAsk.value)
        assertFalse(v.discardable.value)
        // It asks again by itself; the server answers: the code is still waiting.
        advanceTimeBy(2_001)
        runCurrent()
        assertTrue(v.ui.value is SetupUi.ShowCode)
        assertFalse(v.cannotAsk.value)
    }

    /** Silent on three occasions over more than a day, the last question just now. */
    private fun silentForLong(waiting: SetupPendingViewModel) {
        approval.failWith = IOException("offline")
        for (hours in listOf(3L, 9, 28)) {
            now = now.plusSeconds(hours * 3600)
            waiting.check()
        }
    }

    @Test fun `discarding is checked again when it is asked for`() = runTest {
        val (first, f, _) = lostClaim()
        first.onCleared()
        val waiting = SetupPendingViewModel(owner(f))
        silentForLong(waiting)
        assertTrue(waiting.discardable.value)
        // The server answered meanwhile, noted by another part of the app: the offer on the screen is stale.
        val rec = f.repo.record.value!!
        f.repo.update(rec) { it.copy(unanswered = null) }
        waiting.discard()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
        // The stored record is another one by now (the claim turned out to have made a waiting key device).
        silentForLong(waiting)
        assertTrue(waiting.discardable.value)
        f.repo.update(f.repo.record.value!!) { it.copy(phase = KeyDevicePhase.PENDING, readyAt = now.plusSeconds(3600)) }
        waiting.discard()
        assertEquals(KeyDevicePhase.PENDING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    /** The app's own registration: its claim completes by itself, so the server may hold this key. */
    @Test fun `a lost claim of the app's own registration is never offered to be discarded`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimGate = CompletableDeferred()
        val (first, f) = phone()
        val v = SetupFlowViewModel(first)
        v.start(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.TICKET, ticket = "ticket-secret"))
        v.confirmSubject(activity)
        advanceTimeBy(SetupOwner.CLAIM_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertFalse(f.repo.record.value!!.codeClaim)
        v.onCleared()
        val waiting = SetupPendingViewModel(owner(f))
        silentForLong(waiting)
        assertTrue(waiting.cannotAsk.value)
        assertFalse(waiting.discardable.value)
        assertTrue(waiting.lastResort.value)
        waiting.discard()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
    }

    @Test fun `a claim stored by a build that did not say which kind it is is never offered to be discarded`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        f.keyStore.generate(ByteArray(32), use)
        f.repo.seed(
            io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
                LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null, "Oma"),
                KeyDevicePhase.CLAIMING, keyId = f.keyStore.keyId(),
            ),
        )
        val waiting = SetupPendingViewModel(owner)
        silentForLong(waiting)
        assertFalse(waiting.discardable.value)
        assertTrue(waiting.lastResort.value)
        waiting.discard()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
    }

    /** "No key device" without the server's time cannot be counted by: it is not an answer the phone can use. */
    @Test fun `a lost code claim whose server gives no time counts as not answered`() = runTest {
        val (first, f, _) = lostClaim()
        assertTrue(f.repo.record.value!!.codeClaim)
        first.onCleared()
        val waiting = SetupPendingViewModel(owner(f))
        approval.failWith = noKeyDeviceAt(null)
        now = now.plusSeconds(3 * 3600)
        waiting.check()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), waiting.ui.value)
        assertTrue(waiting.cannotAsk.value)
        assertFalse(waiting.discardable.value)
        assertFalse(waiting.lastResort.value)
        for (hours in listOf(6L, 20)) {
            now = now.plusSeconds(hours * 3600)
            waiting.check()
        }
        assertTrue(waiting.discardable.value)
        assertFalse(waiting.lastResort.value)
        waiting.discard()
        assertEquals(SetupUi.Ended(R.string.setup_discarded, canRetry = false), waiting.ui.value)
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
        assertFalse(waiting.cannotAsk.value)
    }

    // ── The server ────────────────────────────────────────────────────────────────────────────

    @Test fun `a server question answered late adopts nothing once the app is tied to another server`() = runTest {
        val (owner, _) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry.copy(origin = "https://other.example.org"))
        assertEquals(SetupUi.ConfirmServer("Familie Müller", "other.example.org"), v.ui.value)
        // While the question is open the app gets tied to a server (a sign-in, a key).
        env.pinned = SigFixtures.ORIGIN
        v.confirmServer()
        assertTrue(env.saved.isEmpty())
        assertNull(env.stored)
        assertEquals(SetupUi.Ended(R.string.setup_other_server, canRetry = false), v.ui.value)
        assertFalse(enroll.calls.contains("popChallenge"))
    }

    @Test fun `a server question answered late adopts nothing once a claim is stored`() = runTest {
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry.copy(origin = "https://other.example.org"))
        assertTrue(v.ui.value is SetupUi.ConfirmServer)
        f.keyStore.generate(ByteArray(32), use)
        f.repo.saveClaimed(LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", "claim-secret", "Oma"))
        v.confirmServer()
        assertTrue(env.saved.isEmpty())
        assertEquals(SetupUi.Resume, v.ui.value)
        assertEquals(SigFixtures.ORIGIN, f.repo.pinnedOrigin())
    }

    // ── The final sign-in ─────────────────────────────────────────────────────────────────────

    @Test fun `the screen leaves the final sign-in at its limit, whatever the sign-in still has to finish`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        var revoked = false
        val (owner, _) = phone()
        coEvery { deviceSignIn.signIn(any(), any()) } coAnswers {
            try {
                awaitCancellation()
            } finally {
                // Ending the session it made but did not install takes its own time.
                withContext(NonCancellable) {
                    delay(10_000)
                    revoked = true
                }
            }
        }
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        v.signIn(activity)
        advanceTimeBy(SetupOwner.SIGN_IN_TIMEOUT_MS - 1)
        assertEquals(SetupUi.SigningIn, v.ui.value)
        advanceTimeBy(2)
        runCurrent()
        assertEquals(SetupUi.SignInTimedOut, v.ui.value)
        assertFalse(revoked)
        // The clean-up finishes on the owner's scope, also when the screen has gone meanwhile.
        v.onCleared()
        advanceTimeBy(10_000)
        runCurrent()
        assertTrue(revoked)
    }

    // ── Discarding is the last step, and only for a setup that can never finish ───────────────

    /**
     * An existing account's phone waits for its ready time and is first opened two days later,
     * without a connection. By then the server has most likely made it a key: nothing on the
     * screen deletes that key, however long and however often the server cannot be asked.
     */
    @Test fun `a waiting key device opened offline long after its ready time is never offered to be discarded`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        f.keyStore.generate(ByteArray(32), use)
        val key = f.keyStore.keyId()
        val readyAt = now.plusSeconds(24 * 3600)
        // As a setup from an admin's link leaves it: claimed with a code, confirmed, waiting for its time.
        f.repo.seed(
            io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
                LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null, "Oma"),
                KeyDevicePhase.PENDING, readyAt = readyAt, keyId = key, installId = SigFixtures.DEVICE_ID,
                since = now, codeClaim = true,
            ),
        )
        now = readyAt.plusSeconds(26 * 3600)
        approval.failWith = IOException("offline")
        val waiting = SetupPendingViewModel(owner)
        waiting.check()
        // The screen says the server could not be asked, keeps the ready time, and offers nothing else.
        assertEquals(SetupUi.Pending(readyAt), waiting.ui.value)
        assertTrue(waiting.cannotAsk.value)
        assertFalse(waiting.discardable.value)
        assertFalse(waiting.lastResort.value)
        waiting.discard()
        assertEquals(KeyDevicePhase.PENDING, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        repeat(8) {
            now = now.plusSeconds(8 * 3600)
            waiting.check()
            assertTrue(waiting.cannotAsk.value)
            assertFalse(waiting.discardable.value)
            waiting.discard()
        }
        // After days of silence the last resort is named, in words; still no action deletes the key.
        assertTrue(waiting.lastResort.value)
        assertEquals(KeyDevicePhase.PENDING, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        assertFalse(f.store.writes.contains("clear"))
        // The server is back: the phone is a key, as the server says.
        approval.failWith = null
        approval.nextChallenge = activeChallenge
        waiting.check()
        assertEquals(SetupUi.SigningIn, waiting.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        assertFalse(waiting.cannotAsk.value)
        assertFalse(waiting.lastResort.value)
        assertNull(f.repo.record.value!!.unanswered)
    }

    /** A code that was shown may have been confirmed while the app was closed. */
    @Test fun `a claim whose code was shown is never offered to be discarded`() = runTest {
        val (waiting, f, _) = waitingForCode()
        val key = f.keyStore.keyId()
        repeat(8) {
            now = now.plusSeconds(8 * 3600)
            enroll.statusFailures += IOException("no route to host")
            waiting.check()
            assertTrue(waiting.cannotAsk.value)
            assertFalse(waiting.discardable.value)
            waiting.discard()
        }
        assertTrue(waiting.lastResort.value)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
    }

    /**
     * A claim that needs a code and never got its answer can never finish (its code was never
     * shown). Even so the offer comes only once its server stayed silent on several separate
     * occasions over at least a day; one failed question, however late, offers nothing.
     */
    @Test fun `a lost code claim may be discarded only after its server stayed silent on several occasions over a day`() = runTest {
        val (first, f, _) = lostClaim()
        first.onCleared()
        val waiting = SetupPendingViewModel(owner(f)) // after a restart: the link is gone
        val key = f.keyStore.keyId()
        approval.failWith = IOException("offline")
        // One failed question, hours after the claim's own time.
        now = now.plusSeconds(3 * 3600)
        val firstFailure = now
        waiting.check()
        assertTrue(waiting.cannotAsk.value)
        assertFalse(waiting.discardable.value)
        waiting.discard()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        // Asked again a few minutes later: the same occasion.
        now = now.plusSeconds(600)
        waiting.check()
        assertFalse(waiting.discardable.value)
        // A second and a third occasion, but less than a day since the first.
        now = firstFailure.plusSeconds(6 * 3600)
        waiting.check()
        assertFalse(waiting.discardable.value)
        now = firstFailure.plusSeconds(12 * 3600)
        waiting.check()
        assertFalse(waiting.discardable.value)
        waiting.discard()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        // More than a day after the first failed question, still silent: now it is offered.
        assertFalse(waiting.lastResort.value)
        now = firstFailure.plusSeconds(24 * 3600 + 60)
        waiting.check()
        assertTrue(waiting.cannotAsk.value)
        assertTrue(waiting.discardable.value)
        assertFalse(waiting.lastResort.value)
        // Offering it changes nothing by itself.
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        waiting.discard()
        assertEquals(SetupUi.Ended(R.string.setup_discarded, canRetry = false), waiting.ui.value)
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `an answer of the server in between starts the count of silent occasions again`() = runTest {
        val (first, f, _) = lostClaim()
        first.onCleared()
        val until = f.repo.record.value!!.claimVoidAfter!!
        val waiting = SetupPendingViewModel(owner(f))
        val t0 = now
        approval.failWith = IOException("offline")
        now = t0.plusSeconds(3 * 3600)
        waiting.check()
        now = t0.plusSeconds(9 * 3600)
        waiting.check()
        assertFalse(waiting.discardable.value)
        // The server answers ("no key device yet", by its own clock still within the claim's time).
        approval.failWith = noKeyDeviceAt(until.minusSeconds(60))
        now = t0.plusSeconds(10 * 3600)
        waiting.check()
        assertEquals(SetupUi.ClaimWaiting, waiting.ui.value)
        assertFalse(waiting.cannotAsk.value)
        // Silent again, a day and more after the very first failed question: the count started anew.
        approval.failWith = IOException("offline")
        now = t0.plusSeconds(28 * 3600)
        waiting.check()
        assertTrue(waiting.cannotAsk.value)
        assertFalse(waiting.discardable.value)
        now = t0.plusSeconds(34 * 3600)
        waiting.check()
        assertFalse(waiting.discardable.value)
        waiting.discard()
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
        // Three occasions over a day, counted from the first one after that answer.
        now = t0.plusSeconds(53 * 3600)
        waiting.check()
        assertTrue(waiting.discardable.value)
    }

    /** The session was installed, and the sign-in was still finishing when the screen's limit hit: it is a sign-in. */
    @Test fun `a sign-in that got through just as the limit hit counts as signed in`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        val (owner, f) = phone()
        coEvery { deviceSignIn.signIn(any(), any()) } coAnswers {
            delay(SetupOwner.SIGN_IN_TIMEOUT_MS - 1)
            f.authState.setLoggedIn(FakeEnrollmentApi.SUBJECT_ID)
            awaitCancellation()
        }
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        v.signIn(activity)
        advanceTimeBy(SetupOwner.SIGN_IN_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.Done, v.ui.value)
    }
}
