package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.keyId
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Duration
import java.time.Instant

/**
 * Every answer the server can give to a request about this phone's key, per request, and what it
 * does to the key. The lists are taken from the server (`server/src/approvals/error.rs`,
 * `server/src/auth/error.rs`, and the handlers named at each list); the expectation is written
 * here independently of the table.
 */
class KeyAnswersTest {

    private val origin = "https://chat.example.com"
    private val use = UvPolicy.choose(30, true, true) as UvDecision.Use

    private class Phone(val repo: KeyDeviceRepository, val keyStore: SoftwareApprovalKeyStore, val answers: KeyAnswers)

    /** The phone's running time. */
    private val boot = io.wolftown.kaiku.testutil.FakeBootClock(boot = 1, elapsedMs = 0)

    private fun TestScope.phone(
        phase: KeyDevicePhase,
        secret: String? = null,
        voidAfter: Instant? = null,
        codeClaim: Boolean = false,
        written: BootTime? = null,
        installId: String? = "22222222-2222-4222-8222-222222222222",
    ): Pair<Phone, StoredKeyDevice> {
        val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), use) }
        val store = InMemoryKeyDeviceStore()
        val rec = StoredKeyDevice(
            LocalKeyDevice(if (phase == KeyDevicePhase.ACTIVE) "kd1" else null, "u1", origin, UvClass.PER_USE, "e1", secret),
            phase, keyId = keyStore.keyId(), attemptId = "a1", claimVoidAfter = voidAfter, codeClaim = codeClaim, written = written, installId = installId,
        )
        store.stored = rec
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        return Phone(repo, keyStore, KeyAnswers(repo, boot)) to rec
    }

    /** `ApprovalError` and the `AuthError`s these routes can end in, as (status, code). */
    private val approvalCodes = listOf(
        404 to "APPROVAL_NOT_FOUND", 409 to "APPROVAL_STATE", 410 to "APPROVAL_EXPIRED", 400 to "SIGNATURE_INVALID",
        403 to "APPROVAL_NOT_ALLOWED", 403 to "KEY_DEVICE_REQUIRED", 404 to "KEY_DEVICE_UNKNOWN", 409 to "KEY_DEVICE_PENDING",
        403 to "KEY_DEVICE_REVOKED", 404 to "ENROLLMENT_TOKEN_INVALID", 409 to "ENROLLMENT_ALREADY_CLAIMED",
        400 to "ENROLL_CODE_WRONG", 410 to "ENROLLMENT_FAILED", 409 to "ENROLLMENT_STATE", 409 to "ENROLLMENT_LIMIT",
        400 to "ENROLLMENT_DEVICE_IN_USE", 400 to "DEVICE_UNSUITABLE", 403 to "ACCOUNT_BANNED", 404 to "LOGIN_CODE_STALE",
        409 to "LOGIN_ATTEMPT_CLAIMED", 409 to "LOGIN_ATTEMPT_OTHER_ACCOUNT", 400 to "APPROVAL_RESPONSE_MISMATCH",
        400 to "VALIDATION_ERROR", 503 to "SERVICE_UNAVAILABLE", 500 to "INTERNAL_ERROR",
    )
    private val authCodes = listOf(
        401 to "INVALID_TOKEN", 401 to "TOKEN_EXPIRED", 401 to "MISSING_AUTH", 401 to "INVALID_AUTH_HEADER", 401 to "TOKEN_ERROR",
        401 to "SESSION_REUSED", 401 to "SESSION_EXPIRED", 403 to "FULL_SESSION_REQUIRED", 403 to "STEP_UP_REQUIRED",
        403 to "PASSWORD_CHANGE_REQUIRED", 403 to "NOT_ADMIN", 409 to "LAST_STRONG_AUTHENTICATOR", 404 to "REGISTRATION_TICKET_INVALID",
        409 to "ACCOUNT_REQUEST_NOT_READY", 429 to "TOO_MANY_ATTEMPTS", 503 to "SERVICE_UNAVAILABLE", 500 to "INTERNAL_ERROR",
    )
    /** What is not the server's contract at all: a proxy, a cache, a captive portal, a fault. */
    private val strange = listOf(
        ServerAnswer(400), ServerAnswer(401), ServerAnswer(403), ServerAnswer(404), ServerAnswer(409), ServerAnswer(410), ServerAnswer(422),
        ServerAnswer(500), ServerAnswer(502), ServerAnswer(503), ServerAnswer(504), ServerAnswer(200), ServerAnswer(202), ServerAnswer(204),
        ServerAnswer(404, "NOT_FOUND"), ServerAnswer(400, "SOMETHING_NEW"), ServerAnswer(200, "KEY_DEVICE_UNKNOWN"),
        ServerAnswer(503, "KEY_DEVICE_UNKNOWN"), ServerAnswer(403, "KEY_DEVICE_UNKNOWN"), ServerAnswer(404, "KEY_DEVICE_REVOKED"),
        ServerAnswer(500, "KEY_DEVICE_REVOKED"), ServerAnswer(410, "KEY_DEVICE_REVOKED"), ServerAnswer(400, "ENROLLMENT_ALREADY_CLAIMED"),
        ServerAnswer(409, "SIGNATURE_INVALID"), ServerAnswer(404, "VALIDATION_ERROR"), ServerAnswer(500, "APPROVAL_EXPIRED"),
        ServerAnswer(202, state = "expired"), ServerAnswer(201, state = "cancelled"), ServerAnswer(404, state = "failed"),
        ServerAnswer(200, state = "done"), ServerAnswer(200, state = "claimed"), ServerAnswer(200, state = "issued"), ServerAnswer(200, state = "unknown"),
    )

    private fun all(vararg lists: List<Pair<Int, String>>): List<ServerAnswer> =
        lists.flatMap { l -> l.map { (s, c) -> ServerAnswer(s, c) } }.distinct() + strange

    private fun expectOnly(ask: Ask, expected: Map<ServerAnswer, Verdict>) {
        for (a in all(approvalCodes, authCodes) + expected.keys) {
            assertEquals("$ask $a", expected[a] ?: Verdict.KEEP, KeyAnswers.verdict(ask, a))
        }
    }

    private val gone = mapOf(ServerAnswer(404, "KEY_DEVICE_UNKNOWN") to Verdict.KEY_GONE, ServerAnswer(403, "KEY_DEVICE_REVOKED") to Verdict.KEY_GONE)

    // server/src/approvals/device_login.rs `challenge`: 201; 404 KEY_DEVICE_UNKNOWN, 403 KEY_DEVICE_REVOKED, 403 ACCOUNT_BANNED,
    // 409 KEY_DEVICE_PENDING, 410 APPROVAL_EXPIRED, 400 VALIDATION_ERROR, 429, 500.
    @Test fun `device-login challenge - only the two key-device answers say the key is gone`() = expectOnly(Ask.DEVICE_LOGIN_CHALLENGE, gone)

    // `sign_in`: 200; 404 APPROVAL_NOT_FOUND, 404 KEY_DEVICE_UNKNOWN, 403 KEY_DEVICE_REVOKED, 403 ACCOUNT_BANNED,
    // 409 KEY_DEVICE_PENDING, 409 APPROVAL_STATE, 410 APPROVAL_EXPIRED, 400 SIGNATURE_INVALID, 503, 500.
    @Test fun `device-login finish - only the two key-device answers say the key is gone`() = expectOnly(Ask.DEVICE_LOGIN, gone)

    // server/src/approvals/handlers.rs `step_up` and `answer`: 201 / 200; 400 VALIDATION_ERROR, 403 KEY_DEVICE_REQUIRED,
    // 403 ACCOUNT_BANNED, 403 FULL_SESSION_REQUIRED, 401 INVALID_TOKEN, 404 APPROVAL_NOT_FOUND, 409 APPROVAL_STATE,
    // 410 APPROVAL_EXPIRED, 400 SIGNATURE_INVALID, 503, 500. None of them is about the key device being gone.
    @Test fun `step-up - no answer costs the key`() = expectOnly(Ask.STEP_UP, emptyMap())

    // server/src/approvals/devices/handlers.rs `remove`: 200; 403 FULL_SESSION_REQUIRED, 404 KEY_DEVICE_UNKNOWN,
    // 403 STEP_UP_REQUIRED, 403 KEY_DEVICE_REVOKED, 409 LAST_STRONG_AUTHENTICATOR, 401, 503, 500.
    @Test fun `removal - the removal itself and the two key-device answers`() =
        expectOnly(Ask.KEY_DEVICE_REMOVE, gone + (ServerAnswer(200) to Verdict.REMOVED))

    // server/src/approvals/enrollments/mod.rs `claim`: 201; 400 VALIDATION_ERROR (before anything is read);
    // 404 ENROLLMENT_TOKEN_INVALID and 409 ENROLLMENT_ALREADY_CLAIMED (first read without the lock);
    // under the enrolment's lock, still open: 404 APPROVAL_NOT_FOUND, 410 APPROVAL_EXPIRED, 409 APPROVAL_STATE,
    // 400 SIGNATURE_INVALID, 409 ENROLLMENT_STATE, 400 DEVICE_UNSUITABLE, 400 ENROLLMENT_DEVICE_IN_USE,
    // 403 APPROVAL_NOT_ALLOWED; 429, 503, 500.
    @Test fun `claim - refused for good, undecided, or kept`() = expectOnly(
        Ask.CLAIM,
        mapOf(
            ServerAnswer(400, "VALIDATION_ERROR") to Verdict.CLAIM_REFUSED,
            ServerAnswer(400, "SIGNATURE_INVALID") to Verdict.CLAIM_REFUSED,
            ServerAnswer(400, "DEVICE_UNSUITABLE") to Verdict.CLAIM_REFUSED,
            ServerAnswer(404, "APPROVAL_NOT_FOUND") to Verdict.CLAIM_REFUSED,
            ServerAnswer(410, "APPROVAL_EXPIRED") to Verdict.CLAIM_REFUSED,
            ServerAnswer(409, "ENROLLMENT_ALREADY_CLAIMED") to Verdict.CLAIM_UNDECIDED,
            ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID") to Verdict.CLAIM_UNDECIDED,
            ServerAnswer(409, "APPROVAL_STATE") to Verdict.CLAIM_UNDECIDED,
            ServerAnswer(409, "ENROLLMENT_STATE") to Verdict.CLAIM_UNDECIDED,
        ),
    )

    // `pop_challenge`: 201; 400 VALIDATION_ERROR, 404 ENROLLMENT_TOKEN_INVALID, 409 ENROLLMENT_ALREADY_CLAIMED (both under
    // the lock), 409 ENROLLMENT_STATE, 400 ENROLLMENT_DEVICE_IN_USE, 410 APPROVAL_EXPIRED, 429, 500.
    @Test fun `claim check - a new challenge voids the claim, an unknown or a claimed enrolment is only that`() = expectOnly(
        Ask.CLAIM_CHECK,
        mapOf(
            ServerAnswer(201) to Verdict.CLAIM_VOID,
            ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID") to Verdict.ENROLMENT_UNKNOWN,
            ServerAnswer(409, "ENROLLMENT_ALREADY_CLAIMED") to Verdict.CLAIM_TAKEN,
        ),
    )

    // `status`: 200 {state: claimed | done | cancelled | expired | failed}; 404 ENROLLMENT_TOKEN_INVALID; 429, 500.
    @Test fun `enrolment status - only 200 with an ending state, or the unknown enrolment`() = expectOnly(
        Ask.ENROLMENT_STATUS,
        mapOf(
            ServerAnswer(200, state = "cancelled") to Verdict.ENROLMENT_ENDED,
            // Worked out from the server's clock: its own verdict, which forgets nothing by itself.
            ServerAnswer(200, state = "expired") to Verdict.ENROLMENT_RAN_OUT,
            ServerAnswer(200, state = "failed") to Verdict.ENROLMENT_ENDED,
            ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID") to Verdict.ENROLMENT_UNKNOWN,
        ),
    )

    @Test fun `every listed answer is covered above and no ask is missing`() {
        assertEquals(Ask.entries.toSet() - Ask.STEP_UP, KeyAnswers.rows.map { it.ask }.toSet())
        assertEquals(23, KeyAnswers.rows.size)
    }

    /** For every ask and every answer: the record and the key are still there unless the row says otherwise for that phase. */
    @Test fun `what is forgotten, per ask, answer and phase`() = runTest {
        val forgets = mapOf(
            Verdict.KEY_GONE to setOf(KeyDevicePhase.ACTIVE, KeyDevicePhase.PENDING),
            Verdict.REMOVED to setOf(KeyDevicePhase.ACTIVE),
            Verdict.CLAIM_REFUSED to setOf(KeyDevicePhase.CLAIMING),
            Verdict.CLAIM_VOID to setOf(KeyDevicePhase.CLAIMING),
            Verdict.ENROLMENT_ENDED to setOf(KeyDevicePhase.CLAIMED),
        )
        for (ask in Ask.entries) for (a in all(approvalCodes, authCodes) + KeyAnswers.rows.map { ServerAnswer(it.status, it.code, it.state) }) {
            for (phase in KeyDevicePhase.entries) {
                val (p, rec) = phone(phase, secret = if (phase == KeyDevicePhase.CLAIMED) "s" else null)
                val settled = p.answers.settle(rec, origin, ask, a, askedEnrolment = "e1")
                val expected = phase in forgets[KeyAnswers.verdict(ask, a)].orEmpty()
                assertEquals("$ask $a $phase", expected, settled.forgotten)
                assertEquals("$ask $a $phase", expected, p.repo.record.value == null)
                assertEquals("$ask $a $phase", expected, !p.keyStore.hasKey())
            }
        }
    }

    @Test fun `an answer from another server than the record's own, or not over https, counts for nothing`() = runTest {
        for (asked in listOf("https://other.example.com", "http://chat.example.com", "https://chat.example.com:8443", null)) {
            val (p, rec) = phone(KeyDevicePhase.ACTIVE)
            assertEquals(KeyAnswers.Settled.Kept(Verdict.KEEP), p.answers.settle(rec, asked, Ask.DEVICE_LOGIN_CHALLENGE, ServerAnswer(404, "KEY_DEVICE_UNKNOWN")))
            assertNotNull(p.repo.record.value)
        }
        // A record that names a plain-http server never loses its key to an answer.
        val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), use) }
        val store = InMemoryKeyDeviceStore().apply {
            stored = StoredKeyDevice(LocalKeyDevice("kd1", "u1", "http://chat.example.com", UvClass.PER_USE, "e1", null), KeyDevicePhase.ACTIVE, keyId = keyStore.keyId())
        }
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        assertFalse(KeyAnswers(repo).settle(store.stored!!, "http://chat.example.com", Ask.DEVICE_LOGIN, ServerAnswer(403, "KEY_DEVICE_REVOKED")).forgotten)
    }

    @Test fun `an answer about a record that is no longer the stored one forgets nothing`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMED, secret = "s")
        val newer = rec.copy(device = rec.device.copy(enrollmentId = "e2"), attemptId = "a2")
        p.repo.forget(rec, Forget.SERVER_ANSWER)
        p.keyStore.generate(ByteArray(32), use)
        p.repo.create(newer.copy(keyId = p.keyStore.keyId(), phase = KeyDevicePhase.CLAIMING))
        // A late "expired" for the older enrolment.
        val late = p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "expired"), askedEnrolment = "e1")
        assertFalse(late.forgotten)
        assertEquals("a2", p.repo.record.value!!.attemptId)
        assertTrue(p.keyStore.hasKey())
    }

    @Test fun `an ended enrolment forgets only when it is the record's own enrolment`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMED, secret = "s")
        assertFalse(p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "cancelled"), askedEnrolment = "another").forgotten)
        assertFalse(p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "cancelled")).forgotten)
        assertTrue(p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "cancelled"), askedEnrolment = "e1").forgotten)
    }

    /**
     * "Expired" is worked out from the server's clock while a confirmation sent just in time may
     * still be stored: said once, twice, or three times, it never forgets the key.
     */
    @Test fun `a code that ran out forgets nothing, however often that is said`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMED, secret = "s")
        repeat(3) {
            val r = p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "expired"), askedEnrolment = "e1")
            assertEquals(KeyAnswers.Settled.Kept(Verdict.ENROLMENT_RAN_OUT), r)
        }
        assertNotNull(p.repo.record.value)
        assertTrue(p.keyStore.hasKey())
    }

    /** "The enrolment is not known" is only "not known": with "no key device" it gives up nothing by itself. */
    @Test fun `an enrolment the server does not know gives a claim up no sooner`() = runTest {
        for ((phase, ask) in listOf(KeyDevicePhase.CLAIMED to Ask.ENROLMENT_STATUS, KeyDevicePhase.CLAIMING to Ask.CLAIM_CHECK)) {
            val (p, rec) = phone(phase, secret = "s", voidAfter = Instant.parse("2026-11-20T10:20:00Z"), written = boot.now())
            val unknown = p.answers.settle(rec, origin, ask, ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID"), askedEnrolment = "e1")
            assertEquals(Verdict.ENROLMENT_UNKNOWN, unknown.verdict)
            assertFalse(p.answers.settle(rec, origin, probeNow(rec)).forgotten)
            assertNotNull(p.repo.record.value)
            assertTrue(p.keyStore.hasKey())
        }
    }

    private val claimed = ServerAnswer(409, "ENROLLMENT_ALREADY_CLAIMED")
    private val noKeyDevice = ServerAnswer(404, "KEY_DEVICE_UNKNOWN")

    /** What the key-device probe itself returns for "no key device" when it is asked now. */
    private suspend fun probeNow(rec: StoredKeyDevice, answer: ServerAnswer = noKeyDevice): ServerKeyState.Gone {
        val api = io.wolftown.kaiku.testutil.FakeApprovalApi().apply { failWith = io.wolftown.kaiku.testutil.apiError(answer.status, answer.code!!) }
        return KeyDeviceProbe(api) { "22222222-2222-4222-8222-222222222222" }.probe(rec) as ServerKeyState.Gone
    }

    /**
     * The claim's answer never came (the stored phase says so: no code of it was ever shown), the
     * enrolment says under its lock that it is claimed, and the key device, asked after that
     * answer, is not there: nothing can come of that claim any more, and it is given up at once,
     * without a server time.
     */
    @Test fun `a claim without an answer is given up at once when its enrolment is claimed and a later probe finds no key device`() = runTest {
        for (answer in listOf(noKeyDevice, ServerAnswer(403, "KEY_DEVICE_REVOKED"))) {
            val (p, rec) = phone(KeyDevicePhase.CLAIMING, voidAfter = Instant.parse("2026-11-20T10:20:00Z"))
            val check = p.answers.settle(rec, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept
            assertEquals(Verdict.CLAIM_TAKEN, check.verdict)
            assertNotNull(check.taken)
            // The claim check alone forgets nothing.
            assertNotNull(p.repo.record.value)
            assertTrue(p.keyStore.hasKey())
            val probe = probeNow(rec, answer) // sent now, after the enrolment answered
            assertEquals(KeyAnswers.Settled.Forgotten(Verdict.KEY_GONE), p.answers.settle(rec, origin, probe, taken = check.taken))
            assertNull(p.repo.record.value)
            assertFalse(p.keyStore.hasKey())
        }
    }

    @Test fun `the probe must have been asked after the claim check answered`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMING)
        // Sent before the enrolment answered: a claim completing in between would not show in it.
        val early = probeNow(rec)
        val check = p.answers.settle(rec, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept
        assertFalse(p.answers.settle(rec, origin, early, taken = check.taken).forgotten)
        // "No key device" that did not come from the probe says nothing about when it was asked.
        assertFalse(p.answers.settle(rec, origin, ServerKeyState.Gone(noKeyDevice), taken = check.taken).forgotten)
        assertNotNull(p.repo.record.value)
        assertTrue(p.keyStore.hasKey())
        // The same proof with a probe sent afterwards counts.
        assertTrue(p.answers.settle(rec, origin, probeNow(rec), taken = check.taken).forgotten)
    }

    @Test fun `without the claim check's 'already claimed' no key device alone gives up nothing`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMING)
        // No proof at all.
        assertFalse(p.answers.settle(rec, origin, probeNow(rec)).forgotten)
        // Every other answer of the claim check gives no proof.
        for (other in listOf(
            ServerAnswer(409, "ENROLLMENT_STATE"), ServerAnswer(400, "ENROLLMENT_DEVICE_IN_USE"), ServerAnswer(500, "INTERNAL_ERROR"),
            ServerAnswer(429, "TOO_MANY_ATTEMPTS"), ServerAnswer(409), ServerAnswer(200, "ENROLLMENT_ALREADY_CLAIMED"),
            ServerAnswer(400, "ENROLLMENT_ALREADY_CLAIMED"), ServerAnswer(410, "APPROVAL_EXPIRED"),
        )) {
            assertNull(other.toString(), (p.answers.settle(rec, origin, Ask.CLAIM_CHECK, other) as KeyAnswers.Settled.Kept).taken)
        }
        // "Already claimed" as the claim's own answer is first read without the lock: no proof.
        for (ask in Ask.entries - Ask.CLAIM_CHECK) {
            assertNull(ask.name, (p.answers.settle(rec, origin, ask, claimed, askedEnrolment = "e1") as KeyAnswers.Settled.Kept).taken)
        }
        // Asked on another server: no proof.
        assertNull((p.answers.settle(rec, "https://other.example.com", Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept).taken)
        assertNotNull(p.repo.record.value)
    }

    @Test fun `with that proof only the probe's own 'no key device' gives the claim up`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMING)
        val proof = (p.answers.settle(rec, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept).taken
        val probe = probeNow(rec)
        // The probe's result with any other answer in it.
        for (other in listOf(
            ServerAnswer(409, "KEY_DEVICE_PENDING"), ServerAnswer(403, "ACCOUNT_BANNED"), ServerAnswer(500, "INTERNAL_ERROR"),
            ServerAnswer(404), ServerAnswer(503, "KEY_DEVICE_UNKNOWN"), ServerAnswer(404, "KEY_DEVICE_REVOKED"), ServerAnswer(201),
        )) {
            assertFalse(other.toString(), p.answers.settle(rec, origin, probe.copy(answer = other), taken = proof).forgotten)
        }
        // "No key device" as the answer to another request cannot carry the proof at all.
        assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN, noKeyDevice).forgotten)
        assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice).forgotten)
        assertFalse(p.answers.settle(rec, origin, Ask.KEY_DEVICE_REMOVE, noKeyDevice).forgotten)
        // The probe's answer from another server.
        assertFalse(p.answers.settle(rec, "https://other.example.com", probe, taken = proof).forgotten)
        assertNotNull(p.repo.record.value)
        assertTrue(p.keyStore.hasKey())
    }

    @Test fun `a claim whose code was received is never given up that way`() = runTest {
        // Stored as received: the claim check gives no proof for it.
        val (p, rec) = phone(KeyDevicePhase.CLAIMED, secret = "s")
        assertNull((p.answers.settle(rec, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept).taken)
        // A proof made while the answer was missing does not count once the answer is stored.
        val (q, claiming) = phone(KeyDevicePhase.CLAIMING)
        val proof = (q.answers.settle(claiming, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept).taken
        val received = (q.repo.replace(claiming, claiming.copy(device = claiming.device.copy(claimSecret = "s"), phase = KeyDevicePhase.CLAIMED)) as Change.Done).record!!
        assertFalse(q.answers.settle(received, origin, probeNow(received), taken = proof).forgotten)
        assertFalse(q.answers.settle(claiming, origin, probeNow(claiming), taken = proof).forgotten)
        assertEquals(KeyDevicePhase.CLAIMED, q.repo.phase())
        assertTrue(q.keyStore.hasKey())
    }

    @Test fun `that proof is about one record and does not carry over to another`() = runTest {
        val (p1, rec1) = phone(KeyDevicePhase.CLAIMING)
        val proof = (p1.answers.settle(rec1, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept).taken
        val (p2, rec2) = phone(KeyDevicePhase.CLAIMING)
        val other = rec2.copy(attemptId = "a2").also { p2.repo.seedForTest(it) }
        assertFalse(p2.answers.settle(other, origin, probeNow(other), taken = proof).forgotten)
        assertNotNull(p2.repo.record.value)
    }

    private val until = Instant.parse("2026-11-20T10:20:00Z")
    private fun goneAt(t: Instant?) = ServerAnswer(404, "KEY_DEVICE_UNKNOWN", serverTime = t)

    /** The probe's own "no key device", sent now, with the server's time [t]. */
    private suspend fun probeAt(rec: StoredKeyDevice, t: Instant?): ServerKeyState.Gone = probeNow(rec).copy(answer = goneAt(t))

    /**
     * An unfinished claim is given up on "no key device" only when all three hold: the server's
     * time is past the claim's time with its margin, the phone's running time since the claim was
     * recorded is past the longest life of a claim with that margin, and a probe sent after both
     * were found still says so. For every kind of unfinished claim.
     */
    @Test fun `an unfinished claim goes only when both times are over and a later probe agrees`() = runTest {
        for ((phase, codeClaim) in listOf(KeyDevicePhase.CLAIMING to true, KeyDevicePhase.CLAIMING to false, KeyDevicePhase.CLAIMED to true)) {
            val case = "$phase codeClaim=$codeClaim"
            boot.elapsedMs = 0
            val (p, rec) = phone(phase, secret = if (phase == KeyDevicePhase.CLAIMED) "s" else null, voidAfter = until, codeClaim = codeClaim, written = boot.now())
            // Both times over: no forgetting yet, only the proof for the next probe.
            boot.advance(KeyAnswers.CLAIM_LIFE_WITH_MARGIN)
            val first = p.answers.settle(rec, origin, probeAt(rec, until.plusSeconds(1))) as KeyAnswers.Settled.Kept
            assertNotNull(case, first.over)
            assertEquals(case, Duration.ZERO, first.askAgainIn)
            assertTrue(case, p.keyStore.hasKey())
            // A probe sent after that, saying "no key device" again: given up.
            assertTrue(case, p.answers.settle(rec, origin, probeAt(rec, until.plusSeconds(30)), over = first.over).forgotten)
            assertFalse(case, p.keyStore.hasKey())
        }
    }

    @Test fun `each of the three conditions missing keeps the key`() = runTest {
        boot.elapsedMs = 0
        val (p, rec) = phone(KeyDevicePhase.CLAIMING, voidAfter = until, codeClaim = false, written = boot.now())
        // 1. The server's time not past (or not said at all).
        boot.advance(Duration.ofHours(5))
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE, askAgainIn = Duration.ofSeconds(120), left = Duration.ofSeconds(120)), p.answers.settle(rec, origin, probeAt(rec, until.minusSeconds(120))))
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE), p.answers.settle(rec, origin, probeAt(rec, null)))
        // 2. The phone's running time not past, whatever the server's time says (a clock ahead).
        val (q, rec2) = phone(KeyDevicePhase.CLAIMING, voidAfter = until, codeClaim = false, written = boot.now())
        boot.advance(Duration.ofMinutes(2))
        val early = q.answers.settle(rec2, origin, probeAt(rec2, until.plus(Duration.ofDays(3))))
        // Asked again every few minutes meanwhile (a late claim is taken over at once).
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE, askAgainIn = KeyAnswers.ASK_AGAIN_WHILE_WAITING, left = Duration.ofMinutes(23)), early)
        // 3. No probe after the proof: the proof's own probe, or one sent before it, does not count.
        boot.advance(Duration.ofMinutes(30))
        val before = probeAt(rec2, until.plusSeconds(60))
        val proof = (q.answers.settle(rec2, origin, probeAt(rec2, until.plusSeconds(60))) as KeyAnswers.Settled.Kept).over
        assertNotNull(proof)
        assertFalse(q.answers.settle(rec2, origin, before, over = proof).forgotten)
        // A proof about another record does not count either.
        val other = rec2.copy(attemptId = "a2")
        assertFalse(q.answers.settle(other, origin, probeAt(other, until.plusSeconds(90)), over = proof).forgotten)
        assertTrue(p.keyStore.hasKey() && q.keyStore.hasKey())
    }

    /** A reboot restarts the phone's count from the start of the new boot; it never shortens the wait. */
    @Test fun `a reboot restarts the phone's count`() = runTest {
        boot.elapsedMs = Duration.ofDays(3).toMillis()
        val (p, rec) = phone(KeyDevicePhase.CLAIMING, voidAfter = until, written = boot.now())
        boot.reboot(Duration.ofMinutes(5))
        val r = p.answers.settle(rec, origin, probeAt(rec, until.plus(Duration.ofHours(1)))) as KeyAnswers.Settled.Kept
        assertNull(r.over)
        assertEquals(KeyAnswers.ASK_AGAIN_WHILE_WAITING, r.askAgainIn)
        boot.advance(Duration.ofMinutes(20))
        assertNotNull((p.answers.settle(rec, origin, probeAt(rec, until.plus(Duration.ofHours(1)))) as KeyAnswers.Settled.Kept).over)
    }

    /** Setting the phone's clock forward or back changes nothing: only its running time counts. */
    @Test fun `the phone's clock does not count, forward or back`() = runTest {
        boot.elapsedMs = 0
        val (p, rec) = phone(KeyDevicePhase.CLAIMING, voidAfter = until, written = boot.now())
        // However the wall clock is set (it is not asked), 10 minutes of running time are 10 minutes.
        boot.advance(Duration.ofMinutes(10))
        val left = p.answers.settle(rec, origin, probeAt(rec, until.plusSeconds(1))) as KeyAnswers.Settled.Kept
        assertNull(left.over)
        assertEquals(KeyAnswers.ASK_AGAIN_WHILE_WAITING, left.askAgainIn)
        // And once both are over the claim is not kept for good.
        boot.advance(Duration.ofMinutes(15))
        val proof = (p.answers.settle(rec, origin, probeAt(rec, until.plusSeconds(1))) as KeyAnswers.Settled.Kept).over
        assertTrue(p.answers.settle(rec, origin, probeAt(rec, until.plusSeconds(2)), over = proof).forgotten)
    }

    @Test fun `a claim stored without its times gets them and counts from there`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMING)
        val t = Instant.parse("2026-11-20T10:00:00Z")
        val first = p.answers.settle(rec, origin, probeAt(rec, t))
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE, askAgainIn = Duration.ofMinutes(20)), first)
        assertEquals(t.plus(Duration.ofMinutes(20)), p.repo.record.value!!.claimVoidAfter)
        val noted = p.repo.record.value!!
        val second = p.answers.settle(noted, origin, probeAt(noted, t.plus(Duration.ofMinutes(21))))
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE, askAgainIn = KeyAnswers.ASK_AGAIN_WHILE_WAITING, left = KeyAnswers.CLAIM_LIFE_WITH_MARGIN), second)
        assertEquals(boot.now(), p.repo.record.value!!.written)
        assertTrue(p.keyStore.hasKey())
    }

    /** "No key device for this install" says nothing about a key whose install id could not be established. */
    @Test fun `a key whose install id is lost is never forgotten on no key device`() = runTest {
        for (phase in listOf(KeyDevicePhase.ACTIVE, KeyDevicePhase.PENDING)) {
            val (p, rec) = phone(phase)
            val lost = rec.copy(installIdLost = true).also { p.repo.seedForTest(it) }
            for (ask in listOf(Ask.DEVICE_LOGIN_CHALLENGE, Ask.DEVICE_LOGIN, Ask.KEY_DEVICE_REMOVE)) {
                assertFalse(p.answers.settle(lost, origin, ask, ServerAnswer(404, "KEY_DEVICE_UNKNOWN")).forgotten)
                assertFalse(p.answers.settle(lost, origin, ask, ServerAnswer(403, "KEY_DEVICE_REVOKED")).forgotten)
            }
            assertTrue(p.keyStore.hasKey())
        }
    }

    /** A record that took the probe's word only provisionally is still the claim it was. */
    @Test fun `a provisional key device is not forgotten as one`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.ACTIVE)
        val provisional = rec.copy(provisionalFrom = KeyDevicePhase.CLAIMING).also { p.repo.seedForTest(it) }
        for (ask in listOf(Ask.DEVICE_LOGIN_CHALLENGE, Ask.DEVICE_LOGIN)) {
            assertFalse(p.answers.settle(provisional, origin, ask, ServerAnswer(404, "KEY_DEVICE_UNKNOWN")).forgotten)
        }
        assertTrue(p.keyStore.hasKey())
    }

    /**
     * A code that arrived with its time: the claim is given up from the code's expiry with a margin
     * of 2 minutes on both clocks (not the 25 minutes since the claim was sent), once a later
     * probe still finds no key device.
     */
    @Test fun `a code that ran out lets go 2 minutes after its own time`() = runTest {
        val codeExpires = Instant.parse("2026-11-20T10:05:00Z")
        boot.elapsedMs = 0
        val (p, base) = phone(KeyDevicePhase.CLAIMED, secret = "s", voidAfter = codeExpires.plus(KeyAnswers.CODE_MARGIN), written = boot.now())
        // The code arrived with 5 minutes left by the server's time: 7 minutes of running time to wait.
        val rec = base.copy(codeReceived = boot.now(), codeWait = Duration.ofMinutes(7)).also { p.repo.seedForTest(it) }
        val serverPast = codeExpires.plus(Duration.ofMinutes(3))
        // Server's time past, phone's not yet: kept, about 4 minutes left.
        boot.advance(Duration.ofMinutes(3))
        val early = p.answers.settle(rec, origin, probeAt(rec, serverPast)) as KeyAnswers.Settled.Kept
        assertNull(early.over)
        assertEquals(Duration.ofMinutes(4), early.left)
        // The server's time is not past yet (1 minute after the expiry): kept.
        boot.advance(Duration.ofMinutes(4))
        assertNull((p.answers.settle(rec, origin, probeAt(rec, codeExpires.plusSeconds(60))) as KeyAnswers.Settled.Kept).over)
        // Both past: the proof; a probe sent after it gives the claim up, 7 minutes after the code arrived.
        val proof = (p.answers.settle(rec, origin, probeAt(rec, serverPast)) as KeyAnswers.Settled.Kept).over
        assertNotNull(proof)
        assertTrue(p.answers.settle(rec, origin, probeAt(rec, serverPast), over = proof).forgotten)
    }

    /**
     * A record stored before a code's wait had its floor, with a wait of 2 minutes: it is read with
     * the floor a new one gets (the server's code lifetime plus the margin, 7 minutes).
     */
    @Test fun `a stored code wait shorter than the floor is read as the floor`() = runTest {
        val codeExpires = Instant.parse("2026-11-20T10:05:00Z")
        boot.elapsedMs = 0
        val (p, base) = phone(KeyDevicePhase.CLAIMED, secret = "s", voidAfter = codeExpires.plus(KeyAnswers.CODE_MARGIN), written = boot.now())
        val rec = base.copy(codeReceived = boot.now(), codeWait = Duration.ofMinutes(2)).also { p.repo.seedForTest(it) }
        val serverPast = codeExpires.plus(Duration.ofMinutes(3))
        boot.advance(Duration.ofMinutes(3))
        assertEquals(Duration.ofMinutes(4), p.answers.ownWaitLeft(rec))
        val early = p.answers.settle(rec, origin, probeAt(rec, serverPast)) as KeyAnswers.Settled.Kept
        assertNull(early.over)
        assertEquals(Duration.ofMinutes(4), early.left)
        boot.advance(Duration.ofMinutes(4))
        val proof = (p.answers.settle(rec, origin, probeAt(rec, serverPast)) as KeyAnswers.Settled.Kept).over
        assertNotNull(proof)
        assertTrue(p.answers.settle(rec, origin, probeAt(rec, serverPast), over = proof).forgotten)
    }

    /** A key whose install id was never recorded: "no key device for this install" forgets nothing either. */
    @Test fun `a key without its install id is never forgotten on no key device`() = runTest {
        for (phase in listOf(KeyDevicePhase.ACTIVE, KeyDevicePhase.PENDING)) {
            val (p, rec) = phone(phase, installId = null)
            for (ask in listOf(Ask.DEVICE_LOGIN_CHALLENGE, Ask.DEVICE_LOGIN, Ask.KEY_DEVICE_REMOVE)) {
                assertFalse(p.answers.settle(rec, origin, ask, ServerAnswer(404, "KEY_DEVICE_UNKNOWN")).forgotten)
                assertFalse(p.answers.settle(rec, origin, ask, ServerAnswer(403, "KEY_DEVICE_REVOKED")).forgotten)
            }
            assertTrue(p.keyStore.hasKey())
        }
    }

    private fun KeyDeviceRepository.seedForTest(r: StoredKeyDevice) {
        val f = KeyDeviceRepository::class.java.getDeclaredField("store").apply { isAccessible = true }
        (f.get(this) as InMemoryKeyDeviceStore).stored = r
        reload()
    }
}
