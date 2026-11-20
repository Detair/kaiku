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
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.delay
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
    private val probe = KeyDeviceProbe(approval) { SigFixtures.DEVICE_ID }
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
        return setupOwner(enroll.api, f.repo, f.signer, deviceSignIn, probe, env) to f
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
     * the first question. With two setups this ended with the second one's claim written over and
     * its key deleted. There is one setup: the link is not used, nothing of it exists.
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
        waiting.check()
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
     * The claim's answer does not come within its limit, but the server stores it. The enrolment
     * then says "already claimed" and, for a moment, the server lists no key device yet.
     * Nothing of that deletes the key: the phone waits, asks again, and goes on.
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
        val until = f.repo.record.value!!.claimVoidAfter!!

        // "Try again": the enrolment is claimed; the key device is not listed yet.
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.failWith = noKeyDeviceAt(until.minusSeconds(300))
        v.retry()
        assertEquals(SetupUi.ClaimWaiting, v.ui.value)
        assertTrue(owner.held())
        assertEquals(key, f.keyStore.keyId())
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())

        // The server has it now: the phone asks again by itself and the setup goes on.
        approval.failWith = null
        approval.nextChallenge = activeChallenge
        advanceTimeBy(300_000 + 6_000)
        runCurrent()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        assertEquals(KeyDevicePhase.ACTIVE, f.repo.phase())
        assertEquals(key, f.keyStore.keyId())
        assertEquals(0, enroll.calls.count { it == "claim" }) // the held request never completed; none was sent again
    }

    @Test fun `a claim that times out and was never stored is given up only after its time`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        enroll.claimGate = CompletableDeferred()
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        v.confirmSubject(activity)
        advanceTimeBy(SetupOwner.CLAIM_TIMEOUT_MS + 1)
        runCurrent()
        val until = f.repo.record.value!!.claimVoidAfter!!
        // Someone else's claim took the link meanwhile; this phone's key is not a key device.
        enroll.popAnswers += apiError(409, "ENROLLMENT_ALREADY_CLAIMED")
        approval.failWith = noKeyDeviceAt(until.minusSeconds(30))
        v.retry()
        assertEquals(SetupUi.ClaimWaiting, v.ui.value)
        assertTrue(f.keyStore.hasKey())
        approval.failWith = noKeyDeviceAt(until.plusSeconds(6))
        advanceTimeBy(30_000 + 6_000)
        runCurrent()
        assertTrue(v.ui.value is SetupUi.Ended)
        assertFalse(f.keyStore.hasKey())
        assertNull(f.repo.local())
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

    @Test fun `the code is shown even when the claimed state could not be stored`() = runTest {
        env.pinned = SigFixtures.ORIGIN
        val (owner, f) = phone()
        val v = SetupFlowViewModel(owner)
        v.start(entry)
        enroll.claimGate = CompletableDeferred()
        v.confirmSubject(activity)
        f.store.failWrites = true // storage fails after the claim was recorded and sent
        enroll.claimGate!!.complete(Unit)
        runCurrent()
        assertEquals("123 456", (v.ui.value as SetupUi.ShowCode).code)
        assertEquals(KeyDevicePhase.CLAIMING, f.repo.phase())
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
}
