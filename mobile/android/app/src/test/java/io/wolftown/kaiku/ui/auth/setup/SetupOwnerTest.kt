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
import io.wolftown.kaiku.testutil.seedAsIs
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.cancel
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
    /** The app's install id cannot be stored (a full disk): asking for it fails, as the real store does. */
    private var installIdNotStored = false
    private val probe = KeyDeviceProbe(approval) { appInstallId }
    private val base: Instant = Instant.parse("2026-11-20T09:00:00Z")
    /** The phone's running time; [now] moves it and the phone's clock together (the tests speak in clock times). */
    private val boot = io.wolftown.kaiku.testutil.FakeBootClock(boot = 1, elapsedMs = 0)
    private var now: Instant
        get() = base.plusMillis(boot.elapsedMs)
        set(v) {
            boot.elapsedMs = java.time.Duration.between(base, v).toMillis()
        }
    private val use = UvPolicy.choose(30, true, true) as UvDecision.Use
    private val activeChallenge = SigFixtures.challengeFor(SigKind.DEVICE_LOGIN, 3, SigFixtures.V03_CONTEXT)

    /** The scopes of the owners this test made: ended after it, so nothing of one test runs into the next. */
    private val ownerScopes = mutableListOf<kotlinx.coroutines.CoroutineScope>()

    /** The app-wide link holder is cleared around every test: a test that fails half way leaves nothing to the next. */
    @Before fun setUp() {
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
        Dispatchers.setMain(UnconfinedTestDispatcher())
    }

    @After fun tearDown() {
        ownerScopes.forEach { it.cancel() }
        ownerScopes.clear()
        Dispatchers.resetMain()
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
    }

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
    private fun owner(f: KeyDeviceFixture): SetupOwner {
        env.session = { (f.authState.session.value as? io.wolftown.kaiku.data.local.AuthSession.LoggedIn)?.sessionId }
        return setupOwner(
            enroll.api, f.repo, f.signer, deviceSignIn, probe, env, clock = { now }, boot = boot, installId = { check(!installIdNotStored) { "push device id not stored" }; appInstallId },
            scope = kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.SupervisorJob() + Dispatchers.Main.immediate).also { ownerScopes += it },
        )
    }

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

    /** The guide's remedy: once the waiting screen says the setup ended, the link opened again works. */
    @Test fun `after the waiting screen reported the end, a link opened again starts`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        f.keyStore.generate(ByteArray(32), use)
        f.repo.saveClaimed(LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", "claim-secret", "Oma"))
        val setupScreen = SetupFlowViewModel(owner)
        setupScreen.start(entry) // a claim is stored: the waiting screen takes over inside the setup screen
        assertEquals(SetupUi.Resume, setupScreen.ui.value)
        val waiting = SetupPendingViewModel(owner)
        enroll.statuses += ClaimStatus(EnrollmentState.CANCELLED)
        waiting.check()
        assertEquals(SetupUi.Ended(R.string.setup_cancelled, canRetry = false), waiting.ui.value)
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

    /**
     * A setup talks to its link's server with absolute addresses and checks its challenges against
     * that server; the app's server address is not read or changed by it, so a setup that is
     * stopped, interrupted or never comes back leaves the sign-in where it was.
     */
    @Test fun `a setup never changes the app's server address`() = runTest {
        val (owner, f) = phone()
        env.stored = "https://typed.example.net" // a server the person had typed in before
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmServer()
        v.confirmSubject(activity)
        assertTrue(v.ui.value is SetupUi.ShowCode)
        assertEquals(listOf(SigFixtures.ORIGIN), enroll.origins.distinct())
        v.cancel()
        v.onCleared()
        assertEquals("https://typed.example.net", env.stored)
        assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
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
        // A stored setup that has not finished owns the entry: the opened link goes nowhere.
        assertFalse(owner.offer(link))
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

    /** The challenge the claim itself answered, handed out again, says nothing about the claim: the key stays. */
    @Test fun `the claim's own challenge handed out again keeps the key`() = runTest {
        val (first, f, owner) = lostClaim()
        val key = f.keyStore.keyId()
        enroll.replayFirstChallenge = true
        first.onCleared()
        val waiting = SetupPendingViewModel(owner)
        waiting.check()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), waiting.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        assertTrue(f.store.writes.none { it == "clear" })
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
        // A fault in front of the server and "too many requests" without the server's code are no
        // answer either; any answer with the server's own code is one, a refusal or "too many" too.
        for (x in listOf(
            io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.BadGateway, "bad gateway"),
            io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.TooManyRequests, "slow down"),
        )) {
            enroll.statusFailures += x
            waiting.check()
            assertTrue(x.toString(), waiting.cannotAsk.value)
        }
        for (x in listOf(apiError(429, "TOO_MANY_ATTEMPTS"), apiError(403, "ACCOUNT_BANNED"))) {
            enroll.statusFailures += IOException("again offline")
            waiting.check()
            assertTrue(waiting.cannotAsk.value)
            enroll.statusFailures += x
            waiting.check()
            assertFalse(x.code, waiting.cannotAsk.value)
            assertNull(x.code, f.repo.record.value!!.unanswered)
        }
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

    /**
     * "No key device" without the server's time cannot be counted by, but it is the server's own
     * answer: the server is there, nothing is noted as silence, and nothing is offered to be discarded.
     */
    @Test fun `a lost code claim whose server gives no time is still an answer`() = runTest {
        val (first, f, _) = lostClaim()
        assertTrue(f.repo.record.value!!.codeClaim)
        first.onCleared()
        val waiting = SetupPendingViewModel(owner(f))
        approval.failWith = noKeyDeviceAt(null)
        for (hours in listOf(3L, 6, 20)) {
            now = now.plusSeconds(hours * 3600)
            waiting.check()
            assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), waiting.ui.value)
            assertFalse(waiting.cannotAsk.value)
            assertFalse(waiting.discardable.value)
        }
        assertNull(f.repo.record.value!!.unanswered)
        assertTrue(f.keyStore.hasKey())
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

    // ── Where a phone with an unfinished setup belongs ───────────────────────────────────────

    /** A phone whose stored setup in [phase] was made by an earlier run of the app (cold start: nothing opened yet). */
    private fun TestScope.storedSetup(phase: KeyDevicePhase): Pair<SetupOwner, KeyDeviceFixture> {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Success
        f.repo.seed(
            io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
                LocalKeyDevice(null, SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", if (phase == KeyDevicePhase.CLAIMED) "secret" else null),
                phase,
                readyAt = if (phase == KeyDevicePhase.PENDING) now.plusSeconds(86_400) else null,
                keyId = f.keyStore.keyId(),
                attemptId = "earlier",
            ),
        )
        return owner(f) to f
    }

    /**
     * Back from the three setup screens that leave as usual while a claim is stored ("not sure",
     * still checking, stopped at the code) never reaches the sign-in form: the stored setup owns
     * the entry, and the sign-in route sends the phone to the waiting screen.
     */
    @Test fun `Back from an unfinished setup never reaches the sign-in form`() = runTest {
        val screens = mutableListOf<Pair<SetupUi, io.wolftown.kaiku.data.keydevice.AppEntry>>()
        // Stopped with the code on screen (CLAIMED).
        run {
            env.pinned = SigFixtures.ORIGIN
            val (owner, f) = phone()
            val v = SetupFlowViewModel(owner)
            v.start(entry)
            v.confirmSubject(activity)
            assertTrue(v.ui.value is SetupUi.ShowCode)
            v.cancel()
            assertEquals(SetupUi.Ended(R.string.setup_stopped_after_code, canRetry = true), v.ui.value)
            assertEquals(KeyDevicePhase.CLAIMED, f.repo.phase())
            screens += v.ui.value to f.repo.entry(hasServer = true)
            v.onCleared()
        }
        // "The connection broke" after the claim (CLAIMING).
        run {
            val (v, f, _) = lostClaim()
            screens += v.ui.value to f.repo.entry(hasServer = true)
            // The claim is still being checked (CLAIMING, the server could still store it).
            val until = f.repo.record.value!!.claimVoidAfter!!
            enroll.popAnswers += apiError(409, "ENROLLMENT_STATE")
            approval.failWith = noKeyDeviceAt(until.minusSeconds(60))
            v.retry()
            assertEquals(SetupUi.ClaimWaiting, v.ui.value)
            screens += v.ui.value to f.repo.entry(hasServer = true)
            v.onCleared()
        }
        assertEquals(3, screens.size)
        for ((ui, where) in screens) {
            // Back (or the screen's own Back button) leaves the setup route ...
            assertEquals(ui.toString(), SetupBack.LEAVE, setupBackAction(ui))
            // ... and the phone still belongs to its waiting screen: the sign-in route moves on at once.
            assertEquals(ui.toString(), io.wolftown.kaiku.data.keydevice.AppEntry.WAITING_SETUP, where)
            assertEquals(ui.toString(), io.wolftown.kaiku.ui.SETUP_PENDING_ROUTE, io.wolftown.kaiku.ui.signedOutRedirect(where, "login"))
        }
    }

    /**
     * A cold start by a setup link for another code while a setup is stored (claim sent, code
     * shown, or waiting for its time): one setup route (the waiting screen), and the notice there.
     */
    @Test fun `a link opened at a cold start over a stored unfinished setup is noted on its waiting screen and not used`() = runTest {
        for (phase in listOf(KeyDevicePhase.CLAIMING, KeyDevicePhase.CLAIMED, KeyDevicePhase.PENDING)) {
            val (owner, f) = storedSetup(phase)
            // The activity offers the link before any screen has opened the setup.
            assertFalse(phase.name, owner.offer(link))
            assertTrue(phase.name, owner.linkIgnored.value)
            // The app opens on the waiting screen only, which shows the notice.
            assertEquals(phase.name, io.wolftown.kaiku.ui.SETUP_PENDING_ROUTE, io.wolftown.kaiku.ui.entryRoute(f.repo.entry(hasServer = true)))
            val waiting = SetupPendingViewModel(owner)
            assertTrue(phase.name, waiting.linkIgnored.value)
            assertFalse(phase.name, waiting.ui.value is SetupUi.Ended)
            assertEquals(phase, f.repo.phase())
            assertTrue(enroll.openTokens.isEmpty())
            waiting.onCleared()
        }
    }

    /** The same after "Try again" on the screen that said the key storage could not be read. */
    @Test fun `a link kept across the unreadable screen is judged against the stored setup`() = runTest {
        for (phase in listOf(KeyDevicePhase.CLAIMING, KeyDevicePhase.CLAIMED, KeyDevicePhase.PENDING)) {
            val (owner, f) = storedSetup(phase)
            io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
            f.store.unreadable = true
            f.repo.reload()
            // Opened while the app could not start: kept, nothing posted.
            assertTrue(io.wolftown.kaiku.navigation.PendingSetupLinks.keep("$ORIGIN_SETUP#v=1&t=$token2"))
            assertNull(io.wolftown.kaiku.navigation.PendingSetupLinks.pending.value)
            // "Try again" reads the store; the kept link goes through the setup owner like any link.
            f.store.unreadable = false
            assertTrue(f.repo.reload())
            io.wolftown.kaiku.navigation.PendingSetupLinks.releaseKept(owner::offer)
            assertNull(phase.name, io.wolftown.kaiku.navigation.PendingSetupLinks.pending.value)
            assertTrue(phase.name, owner.linkIgnored.value)
            val waiting = SetupPendingViewModel(owner)
            assertTrue(phase.name, waiting.linkIgnored.value)
            assertEquals(phase, f.repo.phase())
            waiting.onCleared()
        }
        // Without a stored setup the kept link is posted for the setup screen.
        val (owner, _) = phone()
        assertTrue(io.wolftown.kaiku.navigation.PendingSetupLinks.keep("$ORIGIN_SETUP#v=1&t=$token2"))
        io.wolftown.kaiku.navigation.PendingSetupLinks.releaseKept(owner::offer)
        assertEquals(token2, io.wolftown.kaiku.navigation.PendingSetupLinks.take()?.tokenB64u)
    }

    /**
     * The server-address screen probes a server; meanwhile a setup link opens a setup that waits at
     * its question. When the probe answers, the address may not change: the setup holds an attempt.
     */
    @Test fun `the server address does not change while the setup holds an attempt`() = runTest {
        val (owner, f) = phone()
        val rule = ServerAddressRule(f.repo, { env.pinnedOrigin() }, owner::holdsAttempt)
        assertTrue(rule.mayChange())
        val v = SetupFlowViewModel(owner)
        v.start(link)
        assertTrue(v.ui.value is SetupUi.ConfirmServer)
        assertFalse(rule.mayChange())
        v.confirmServer()
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        assertFalse(rule.mayChange())
        // Once the setup was stopped and its screen closed, the address may change again.
        v.cancel()
        v.onCleared()
        assertTrue(rule.mayChange())
    }

    // ── The claim's answer is about this claim, or it is not known ────────────────────────────

    /**
     * Only the answer this claim can have counts: for this very enrolment, of the kind the claim
     * gets, with every part. Anything else leaves the stored claim exactly as it was, to be checked
     * like a lost one; the record never takes another enrolment's id or secret.
     */
    @Test fun `a claim answer that is not this claim's leaves the claim as it was`() = runTest {
        val good = enroll.claimResult
        for (bad in listOf(
            good.copy(enrollmentId = "66666666-6666-4666-8666-666666666666"),
            good.copy(code = null),
            good.copy(claimSecret = null),
            good.copy(codeExpiresAt = null),
            good.copy(state = "issued"),
            // "Done" is the answer of the app's own registration, not of a setup code.
            io.wolftown.kaiku.domain.model.ClaimResult(good.enrollmentId, "done", authenticatorState = "active"),
        )) {
            env.pinned = SigFixtures.ORIGIN
            enroll.claimResult = bad
            val (owner, f) = phone()
            val v = SetupFlowViewModel(owner)
            v.start(entry)
            v.confirmSubject(activity)
            assertEquals(bad.toString(), SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
            val stored = f.repo.record.value!!
            assertEquals(bad.toString(), KeyDevicePhase.CLAIMING, stored.phase)
            assertEquals(bad.toString(), good.enrollmentId, stored.device.enrollmentId)
            assertNull(bad.toString(), stored.device.claimSecret)
            assertTrue(f.keyStore.hasKey())
            assertFalse(enroll.calls.contains("status"))
            v.onCleared()
            enroll.calls.clear()
        }
    }

    @Test fun `a claim answer of this claim keeps the code's own time with the record`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        assertTrue(v.ui.value is SetupUi.ShowCode)
        val stored = f.repo.record.value!!
        assertEquals(KeyDevicePhase.CLAIMED, stored.phase)
        assertEquals(Instant.parse(enroll.claimResult.codeExpiresAt).plus(io.wolftown.kaiku.data.keydevice.KeyAnswers.CODE_MARGIN), stored.claimVoidAfter)
        assertEquals(boot.now().boot, stored.written!!.boot)
    }

    // ── "This install is a key device" is not "this key is" ───────────────────────────────────

    /**
     * The probe says the install is a key device. That may be an older key of this install: the
     * record takes it only provisionally, and a refused signature of this key sends it back to the
     * claim it was, which never takes that word again.
     */
    @Test fun `a key device found by the probe is provisional, and not taken again once refused`() = runTest {
        val (first, f, _) = lostClaim()
        first.onCleared()
        val waiting = SetupPendingViewModel(owner(f))
        approval.nextChallenge = activeChallenge
        waiting.check()
        assertEquals(SetupUi.SigningIn, waiting.ui.value)
        val adopted = f.repo.record.value!!
        assertEquals(KeyDevicePhase.ACTIVE, adopted.phase)
        assertEquals(KeyDevicePhase.CLAIMING, adopted.provisionalFrom)
        // The server refused this key's signature (DeviceSignInTest): the record is the claim it was again.
        f.repo.replace(adopted, adopted.copy(phase = KeyDevicePhase.CLAIMING, provisionalFrom = null, notThisKey = true))
        // Asked again, the probe's word is not taken a second time.
        val again = SetupPendingViewModel(owner(f))
        approval.nextChallenge = activeChallenge
        again.check()
        assertEquals(SetupUi.Ended(R.string.setup_not_this_key, canRetry = true), again.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertTrue(f.keyStore.hasKey())
        // Once the other key is removed on the server, the phone gets out by itself: the probe
        // then says "no key device" and the rule for unfinished claims applies (times long over).
        boot.advance(io.wolftown.kaiku.data.keydevice.KeyAnswers.CLAIM_LIFE_WITH_MARGIN)
        approval.failWith = noKeyDeviceAt(f.repo.record.value!!.claimVoidAfter!!.plusSeconds(60))
        again.check()
        advanceTimeBy(10_000)
        runCurrent()
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
    }

    // ── One link at a time on its way to the setup screen ─────────────────────────────────────

    /** Two links before the first reached the setup screen: the first keeps its place, the second is noted. */
    @Test fun `a link on its way to the setup screen is not replaced by a later one`() = runTest {
        val (owner, _) = phone()
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
        io.wolftown.kaiku.navigation.PendingSetupLinks.deliver(entry.copy(source = SetupEntry.Source.LINK), owner::offer)
        io.wolftown.kaiku.navigation.PendingSetupLinks.deliver(link, owner::offer)
        assertEquals(token, io.wolftown.kaiku.navigation.PendingSetupLinks.pending.value?.tokenB64u)
        assertTrue(owner.linkIgnored.value)
        // The same link again is not "another" link.
        val again = phone().first
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
        io.wolftown.kaiku.navigation.PendingSetupLinks.deliver(link, again::offer)
        io.wolftown.kaiku.navigation.PendingSetupLinks.deliver(link, again::offer)
        assertFalse(again.linkIgnored.value)
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
    }

    /**
     * The claim answer's `Date` runs 3 minutes ahead (a clock in front of the server): it would
     * leave the code 2 minutes. The phone still waits the server's code lifetime and the margin
     * (5 + 2 minutes of its own time) before it lets go.
     */
    // ── The claim is made for the session that was checked ──────────────────────────────────────

    /** Someone signs in while the question is on screen: confirming sends no claim and keeps nothing. */
    @Test fun `a sign-in that completes while the question is shown stops the claim`() = runTest(timeout = BOUNDED) {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        env.signedInUser = "someone-else"

        v.confirmSubject(activity)

        assertEquals(SetupUi.Ended(R.string.setup_for_someone_else, canRetry = false, arg = "Oma"), v.ui.value)
        assertEquals(0, enroll.calls.count { it == "claim" })
        assertNull(f.repo.local())
        assertTrue(f.store.writes.none { it != "clear" })
        assertFalse(f.keyStore.hasKey())
    }

    /** The app is tied to another server meanwhile: confirming sends no claim and keeps nothing. */
    @Test fun `a sign-in on another server while the question is shown stops the claim`() = runTest(timeout = BOUNDED) {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        env.pinned = "https://other.example.org"
        env.signedInUser = FakeEnrollmentApi.SUBJECT_ID

        v.confirmSubject(activity)

        assertEquals(SetupUi.Ended(R.string.setup_other_server, canRetry = false), v.ui.value)
        assertEquals(0, enroll.calls.count { it == "claim" })
        assertNull(f.repo.local())
        assertFalse(f.keyStore.hasKey())
    }

    @Test fun `a server time running ahead never shortens the wait after a code`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val codeExpires = Instant.parse(enroll.claimResult.codeExpiresAt)
        enroll.claimResult = enroll.claimResult.copy(serverTime = codeExpires.minusSeconds(120))
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.statuses += ClaimStatus(EnrollmentState.EXPIRED)
        approval.failWith = noKeyDeviceAt(codeExpires.plusSeconds(180))
        v.confirmSubject(activity)
        runCurrent()
        assertEquals(SetupUi.CodeRanOut(7), v.ui.value)
        // 4 minutes of the phone's own time: what the answer's `Date` alone would have allowed.
        boot.advance(java.time.Duration.ofMinutes(4))
        advanceTimeBy(5 * 60_000L + 6_000)
        runCurrent()
        advanceTimeBy(6_000)
        runCurrent()
        assertTrue(f.keyStore.hasKey())
        assertTrue(v.ui.value is SetupUi.CodeRanOut)
        // 7 minutes: now it lets go.
        boot.advance(java.time.Duration.ofMinutes(3))
        advanceTimeBy(5 * 60_000L + 6_000)
        runCurrent()
        advanceTimeBy(6_000)
        runCurrent()
        assertFalse(f.keyStore.hasKey())
        assertEquals(SetupUi.Ended(R.string.setup_code_not_confirmed, canRetry = false), v.ui.value)
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
    }

    private companion object {
        const val ORIGIN_SETUP = "https://chat.example.com/setup"
        /** A test whose guard is missing ends red instead of hanging. */
        val BOUNDED = kotlin.time.Duration.parse("30s")
    }

    // ── A code that ran out: the phone says how long it holds on, and keeps a new link ────────

    @Test fun `a code that ran out says how long it waits, keeps a new link, and starts it afterwards`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val codeExpires = Instant.parse(enroll.claimResult.codeExpiresAt)
        // The server answered the claim 5 minutes before the code's expiry.
        enroll.claimResult = enroll.claimResult.copy(serverTime = codeExpires.minusSeconds(300))
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.statuses += ClaimStatus(EnrollmentState.EXPIRED)
        approval.failWith = noKeyDeviceAt(codeExpires.plusSeconds(180))
        v.confirmSubject(activity)
        runCurrent()
        // 5 minutes of the code's life and 2 more, by the phone's running time.
        assertEquals(SetupUi.CodeRanOut(7), v.ui.value)
        assertTrue(f.keyStore.hasKey())
        // A new link for the same server is kept for afterwards, and the screen says so.
        val next = SetupEntry(SigFixtures.ORIGIN, token2, SetupEntry.Source.LINK)
        assertFalse(owner.offer(next))
        assertEquals(7, owner.linkKept.value)
        assertFalse(owner.linkIgnored.value)
        // A link for another server is not kept.
        assertFalse(owner.offer(SetupEntry("https://other.example.org", token, SetupEntry.Source.LINK)))
        assertEquals(7, owner.linkKept.value)
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
        // The time passes; the claim is given up and says why; the kept link goes to the setup screen.
        boot.advance(java.time.Duration.ofMinutes(7))
        advanceTimeBy(5 * 60_000L + 6_000)
        runCurrent()
        advanceTimeBy(6_000)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_code_not_confirmed, canRetry = false), v.ui.value)
        assertFalse(f.keyStore.hasKey())
        assertEquals(token2, io.wolftown.kaiku.navigation.PendingSetupLinks.pending.value?.tokenB64u)
        assertNull(owner.linkKept.value)
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
    }

    /** The setup that is ending, its link tapped again: a new link kept meanwhile stays kept. */
    @Test fun `the ending setup's own link does not replace a kept link`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val codeExpires = Instant.parse(enroll.claimResult.codeExpiresAt)
        enroll.claimResult = enroll.claimResult.copy(serverTime = codeExpires.minusSeconds(300))
        val (owner, _) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.statuses += ClaimStatus(EnrollmentState.EXPIRED)
        approval.failWith = noKeyDeviceAt(codeExpires.plusSeconds(180))
        v.confirmSubject(activity)
        runCurrent()
        assertTrue(v.ui.value is SetupUi.CodeRanOut)
        val next = SetupEntry(SigFixtures.ORIGIN, token2, SetupEntry.Source.LINK)
        assertFalse(owner.offer(next))
        // The old code once more, as a link: nothing changes.
        assertFalse(owner.offer(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.LINK)))
        v.start(SetupEntry(SigFixtures.ORIGIN, token, SetupEntry.Source.LINK))
        boot.advance(java.time.Duration.ofMinutes(7))
        advanceTimeBy(5 * 60_000L + 6_000)
        runCurrent()
        advanceTimeBy(6_000)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_code_not_confirmed, canRetry = false), v.ui.value)
        assertEquals(token2, io.wolftown.kaiku.navigation.PendingSetupLinks.pending.value?.tokenB64u)
        io.wolftown.kaiku.navigation.PendingSetupLinks.clear()
    }

    // ── "Done" counts by the state of the key device it made ──────────────────────────────────

    /** "Done" without the key device's state proves nothing about this key: the claim and its secret stay. */
    @Test fun `a 'done' without a key device state keeps the claim and its secret`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.statuses += ClaimStatus(EnrollmentState.DONE)
        approval.failWith = IOException("offline")
        v.confirmSubject(activity)
        runCurrent()
        val rec = f.repo.record.value!!
        assertEquals(KeyDevicePhase.CLAIMED, rec.phase)
        assertEquals("claim-secret", rec.device.claimSecret)
        assertTrue(f.keyStore.hasKey())
        // The key-device probe was asked.
        assertTrue(approval.calls.isNotEmpty())
    }

    /**
     * A key device made and removed again right after the confirmation: the server says so, and the
     * setup ends at once the way a removed key does (no wait for the code's time, no probe).
     */
    @Test fun `a 'done' whose key device was removed ends at once as a removed key`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, _) = phone()
        coEvery { deviceSignIn.signIn(any(), any()) } returns DeviceSignInResult.Revoked
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "revoked")
        v.confirmSubject(activity)
        runCurrent()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        v.signIn(activity)
        runCurrent()
        assertEquals(SetupUi.Ended(io.wolftown.kaiku.ui.auth.signInResultText(DeviceSignInResult.Revoked)!!, canRetry = false), v.ui.value)
        assertTrue(approval.calls.isEmpty())
    }

    // ── "This phone is already a key of this account" ─────────────────────────────────────────

    /** The server still lists this phone, but no key of it is here: no sign-in is offered, the way out is named. */
    @Test fun `a phone still listed is told what to do when the answer comes before the question`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, _) = phone()
        val v = SetupFlowViewModel(owner)
        enroll.popFailure = apiError(409, "ENROLLMENT_DEVICE_IN_USE")
        v.start(entry)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_device_in_use_no_key, canRetry = false), v.ui.value)
    }

    // ── A scanner restored over a stored setup ────────────────────────────────────────────────

    /**
     * A setup started on the scanner screen stores its claim and then hands back to the scanner
     * (its answer was lost, and the person went back): the scanner gives way to the waiting screen.
     */
    @Test fun `a scanner given back after its setup stored a claim gives way to the waiting screen`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimGate = CompletableDeferred()
        val (owner, f) = phone()
        val scanner = SetupFlowViewModel(owner)
        assertFalse(scanner.scannerGivesWay.value)
        scanner.start(entry)
        scanner.confirmSubject(activity)
        advanceTimeBy(SetupOwner.CLAIM_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), scanner.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        scanner.reset()
        assertFalse(scanner.startedFlow.value)
        assertTrue(scanner.scannerGivesWay.value)
    }

    /** After a restart, a scanner screen that was on top while a setup is stored shows the waiting screen instead. */
    @Test fun `a restored scanner gives way to the waiting screen of a stored setup`() = runTest {
        val (first, f, _) = lostClaim()
        first.onCleared()
        // A new process: a new owner, a scanner screen restored without its setup.
        val restored = SetupFlowViewModel(owner(f))
        assertTrue(restored.storedSetupWaits())
        // A phone without a stored setup keeps its scanner.
        val (fresh, _) = phone()
        assertFalse(SetupFlowViewModel(fresh).storedSetupWaits())
    }

    /** A legacy waiting key device whose install id is lost: nothing can be asked; the screen names the way out. */
    @Test fun `a waiting key device whose install id is lost names the way out`() = runTest {
        val (owner, f) = storedSetup(KeyDevicePhase.PENDING)
        f.repo.seedAsIs(f.repo.record.value!!.copy(installId = null, installIdLost = true))
        val waiting = SetupPendingViewModel(owner)
        waiting.check()
        assertEquals(SetupUi.Ended(R.string.confirm_install_unknown, canRetry = false), waiting.ui.value)
        assertTrue(approval.calls.isEmpty())
        assertTrue(f.keyStore.hasKey())
    }

    /** The install id cannot be stored: the setup says so and offers to try again; it neither crashes nor claims anything. */
    @Test fun `a setup whose install id cannot be stored says so and starts on try again`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        installIdNotStored = true
        v.start(entry)
        // A write that failed: "free up space, then Try again", and the button says "Try again".
        assertEquals(SetupUi.Ended(R.string.security_key_storage_not_saved, canRetry = true), v.ui.value)
        assertEquals(R.string.common_try_again, endedRetryLabel(R.string.security_key_storage_not_saved))
        assertTrue(enroll.calls.isEmpty())
        assertNull(f.repo.record.value)
        // Still not stored: the same again.
        v.retry()
        assertEquals(SetupUi.Ended(R.string.security_key_storage_not_saved, canRetry = true), v.ui.value)
        // Stored now: the same link starts.
        installIdNotStored = false
        v.retry()
        assertTrue(v.ui.value is SetupUi.ConfirmSubject)
        assertEquals(listOf(token), enroll.openTokens)
    }

    /**
     * The claim check is answered with a challenge that only repeats the claim's own: it proves
     * nothing about the claim, but it is an answer of the server, so nothing is noted against it.
     */
    @Test fun `a challenge that repeats the claim's own still counts as an answer of the server`() = runTest {
        val (v, f, _) = lostClaim()
        enroll.popFailure = IOException("offline")
        v.retry()
        runCurrent()
        assertEquals(1, f.repo.record.value!!.unanswered!!.occasions)
        enroll.popFailure = null
        enroll.replayFirstChallenge = true
        v.retry()
        runCurrent()
        assertEquals(SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true), v.ui.value)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
        assertNull(f.repo.record.value!!.unanswered)
    }

    // ── A setup is finished only by its own device sign-in ───────────────────────────────────

    /**
     * The phone is already signed in to the same account (a passkey) while it is set up. The final
     * sign-in's prompt never returns: the screen says it took too long; the session that was there
     * before is no proof that this sign-in went through.
     */
    @Test fun `an existing session of the account does not finish a final sign-in that never returned`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")
        val (owner, f) = phone()
        f.authState.setLoggedIn(FakeEnrollmentApi.SUBJECT_ID)
        env.signedInUser = FakeEnrollmentApi.SUBJECT_ID
        coEvery { deviceSignIn.signIn(any(), any()) } coAnswers { awaitCancellation() }
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        runCurrent()
        v.signIn(activity)
        advanceTimeBy(SetupOwner.SIGN_IN_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.SignInTimedOut, v.ui.value)
    }

    /**
     * Cold start: the setup screen comes back without its code; the stored key is a key device only
     * by the server's word about this install (provisional), and a session of the same account
     * exists. The setup is not finished: it goes back to the setup (the waiting screen, which signs
     * in with this key), not home. A key whose own signature was accepted is done.
     */
    @Test fun `a restored setup whose key is only provisionally active goes back to the setup`() = runTest {
        for (provisional in listOf(KeyDevicePhase.CLAIMED, null)) {
            val f = KeyDeviceFixture(backgroundScope, active = false)
            f.repo.seed(
                io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
                    LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null),
                    KeyDevicePhase.ACTIVE,
                    keyId = f.keyStore.keyId(),
                    attemptId = "earlier",
                    provisionalFrom = provisional,
                ),
            )
            f.authState.setLoggedIn(SigFixtures.USER_ID)
            val v = SetupFlowViewModel(owner(f))
            v.resumeWithoutEntry()
            if (provisional != null) {
                assertEquals(SetupUi.Resume, v.ui.value)
                assertTrue(v.waiting.value)
            } else {
                assertEquals(SetupUi.Done, v.ui.value)
            }
        }
    }

    /**
     * A key that is a key device only provisionally: a session installed while its final sign-in
     * ran past the screen's limit does not show that this key's signature was accepted, so the
     * screen does not say the setup is done.
     */
    @Test fun `a session installed meanwhile does not finish a provisional key's final sign-in`() = runTest {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        f.repo.seed(
            io.wolftown.kaiku.data.keydevice.StoredKeyDevice(
                LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null),
                KeyDevicePhase.ACTIVE,
                keyId = f.keyStore.keyId(),
                attemptId = "earlier",
                provisionalFrom = KeyDevicePhase.CLAIMED,
            ),
        )
        coEvery { deviceSignIn.signIn(any(), any()) } coAnswers {
            delay(SetupOwner.SIGN_IN_TIMEOUT_MS - 1)
            f.authState.setLoggedIn(SigFixtures.USER_ID)
            awaitCancellation()
        }
        val owner = owner(f)
        owner.openWaiting()
        assertEquals(SetupUi.SigningIn, owner.ui.value)
        owner.signIn(activity)
        advanceTimeBy(SetupOwner.SIGN_IN_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(SetupUi.SignInTimedOut, owner.ui.value)
    }
}
