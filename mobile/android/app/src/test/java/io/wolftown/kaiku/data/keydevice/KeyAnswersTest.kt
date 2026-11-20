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

    private fun TestScope.phone(phase: KeyDevicePhase, secret: String? = null, voidAfter: Instant? = null): Pair<Phone, StoredKeyDevice> {
        val keyStore = SoftwareApprovalKeyStore().apply { generate(ByteArray(32), use) }
        val store = InMemoryKeyDeviceStore()
        val rec = StoredKeyDevice(
            LocalKeyDevice(if (phase == KeyDevicePhase.ACTIVE) "kd1" else null, "u1", origin, UvClass.PER_USE, "e1", secret),
            phase, keyId = keyStore.keyId(), attemptId = "a1", claimVoidAfter = voidAfter,
        )
        store.stored = rec
        val repo = KeyDeviceRepository(store, keyStore, AuthState(backgroundScope), backgroundScope)
        return Phone(repo, keyStore, KeyAnswers(repo)) to rec
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
            ServerAnswer(200, state = "expired") to Verdict.ENROLMENT_ENDED,
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
        assertFalse(p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "expired"), askedEnrolment = "another").forgotten)
        assertFalse(p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "expired")).forgotten)
        assertTrue(p.answers.settle(rec, origin, Ask.ENROLMENT_STATUS, ServerAnswer(200, state = "expired"), askedEnrolment = "e1").forgotten)
    }

    @Test fun `a claim loses its key to 'no key device' only once the server no longer knows its enrolment`() = runTest {
        val goneAnswer = ServerAnswer(404, "KEY_DEVICE_UNKNOWN")
        for ((phase, ask) in listOf(KeyDevicePhase.CLAIMED to Ask.ENROLMENT_STATUS, KeyDevicePhase.CLAIMING to Ask.CLAIM_CHECK)) {
            val (p, rec) = phone(phase, secret = "s")
            assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, goneAnswer).forgotten)
            val unknown = p.answers.settle(rec, origin, ask, ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID"), askedEnrolment = "e1") as KeyAnswers.Settled.Kept
            assertNotNull(unknown.unknown)
            assertNotNull(p.repo.record.value)
            assertTrue(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, goneAnswer, unknown = unknown.unknown).forgotten)
        }
        // The proof is about one record: it does not carry over to another.
        val (p1, rec1) = phone(KeyDevicePhase.CLAIMED, secret = "s")
        val proof = (p1.answers.settle(rec1, origin, Ask.ENROLMENT_STATUS, ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID"), askedEnrolment = "e1") as KeyAnswers.Settled.Kept).unknown
        val (p2, rec2) = phone(KeyDevicePhase.CLAIMED, secret = "s")
        assertFalse(p2.answers.settle(rec2.copy(attemptId = "a2").also { p2.repo.seedForTest(it) }, origin, Ask.DEVICE_LOGIN_CHALLENGE, goneAnswer, unknown = proof).forgotten)
        // Asked for another enrolment, or from the wrong request: no proof.
        val (p3, rec3) = phone(KeyDevicePhase.CLAIMED, secret = "s")
        assertNull((p3.answers.settle(rec3, origin, Ask.ENROLMENT_STATUS, ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID"), askedEnrolment = "x") as KeyAnswers.Settled.Kept).unknown)
        assertNull((p3.answers.settle(rec3, origin, Ask.CLAIM_CHECK, ServerAnswer(404, "ENROLLMENT_TOKEN_INVALID")) as KeyAnswers.Settled.Kept).unknown)
    }

    private val claimed = ServerAnswer(409, "ENROLLMENT_ALREADY_CLAIMED")
    private val noKeyDevice = ServerAnswer(404, "KEY_DEVICE_UNKNOWN")

    /**
     * The claim's answer never came (the stored phase says so: no code of it was ever shown), the
     * enrolment says under its lock that it is claimed, and the key device, asked after that
     * answer, is not there: nothing can come of that claim any more, and it is given up at once,
     * without a server time.
     */
    @Test fun `a claim without an answer is given up at once when its enrolment is claimed and a later probe finds no key device`() = runTest {
        for (probe in listOf(noKeyDevice, ServerAnswer(403, "KEY_DEVICE_REVOKED"))) {
            val (p, rec) = phone(KeyDevicePhase.CLAIMING, voidAfter = Instant.parse("2026-11-20T10:20:00Z"))
            val check = p.answers.settle(rec, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept
            assertEquals(Verdict.CLAIM_TAKEN, check.verdict)
            assertNotNull(check.taken)
            // The claim check alone forgets nothing.
            assertNotNull(p.repo.record.value)
            assertTrue(p.keyStore.hasKey())
            val asked = AskOrder.next() // the probe is sent now
            assertEquals(
                KeyAnswers.Settled.Forgotten(Verdict.KEY_GONE),
                p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, probe, taken = check.taken, askedAt = asked),
            )
            assertNull(p.repo.record.value)
            assertFalse(p.keyStore.hasKey())
        }
    }

    @Test fun `the probe must have been asked after the claim check answered`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMING)
        // Sent before the enrolment answered: a claim completing in between would not show in it.
        val early = AskOrder.next()
        val check = p.answers.settle(rec, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept
        assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, taken = check.taken, askedAt = early).forgotten)
        // An answer that does not say when it was asked.
        assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, taken = check.taken).forgotten)
        assertNotNull(p.repo.record.value)
        assertTrue(p.keyStore.hasKey())
        // The same proof with a probe sent afterwards counts.
        assertTrue(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, taken = check.taken, askedAt = AskOrder.next()).forgotten)
    }

    @Test fun `without the claim check's 'already claimed' no key device alone gives up nothing`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMING)
        // No proof at all.
        assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, askedAt = AskOrder.next()).forgotten)
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
        for (other in listOf(
            ServerAnswer(409, "KEY_DEVICE_PENDING"), ServerAnswer(403, "ACCOUNT_BANNED"), ServerAnswer(500, "INTERNAL_ERROR"),
            ServerAnswer(404), ServerAnswer(503, "KEY_DEVICE_UNKNOWN"), ServerAnswer(404, "KEY_DEVICE_REVOKED"), ServerAnswer(201),
        )) {
            assertFalse(other.toString(), p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, other, taken = proof, askedAt = AskOrder.next()).forgotten)
        }
        // "No key device" from another request than the probe, or from another server.
        assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN, noKeyDevice, taken = proof, askedAt = AskOrder.next()).forgotten)
        assertFalse(p.answers.settle(rec, origin, Ask.KEY_DEVICE_REMOVE, noKeyDevice, taken = proof, askedAt = AskOrder.next()).forgotten)
        assertFalse(p.answers.settle(rec, "https://other.example.com", Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, taken = proof, askedAt = AskOrder.next()).forgotten)
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
        assertFalse(q.answers.settle(received, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, taken = proof, askedAt = AskOrder.next()).forgotten)
        assertFalse(q.answers.settle(claiming, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, taken = proof, askedAt = AskOrder.next()).forgotten)
        assertEquals(KeyDevicePhase.CLAIMED, q.repo.phase())
        assertTrue(q.keyStore.hasKey())
    }

    @Test fun `that proof is about one record and does not carry over to another`() = runTest {
        val (p1, rec1) = phone(KeyDevicePhase.CLAIMING)
        val proof = (p1.answers.settle(rec1, origin, Ask.CLAIM_CHECK, claimed) as KeyAnswers.Settled.Kept).taken
        val (p2, rec2) = phone(KeyDevicePhase.CLAIMING)
        val other = rec2.copy(attemptId = "a2").also { p2.repo.seedForTest(it) }
        assertFalse(p2.answers.settle(other, origin, Ask.DEVICE_LOGIN_CHALLENGE, noKeyDevice, taken = proof, askedAt = AskOrder.next()).forgotten)
        assertNotNull(p2.repo.record.value)
    }

    @Test fun `a claim without an answer is given up only after its time, by the server's clock`() = runTest {
        val until = Instant.parse("2026-11-20T10:20:00Z")
        val goneAt = { t: Instant? -> ServerAnswer(404, "KEY_DEVICE_UNKNOWN", serverTime = t) }
        val (p, rec) = phone(KeyDevicePhase.CLAIMING, voidAfter = until)
        // No server time: nothing is known about how late it is.
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE), p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, goneAt(null)))
        // Within the time the claim could still be stored: kept, with how long to wait.
        val early = p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, goneAt(until.minusSeconds(120)))
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE, askAgainIn = Duration.ofSeconds(120)), early)
        assertFalse(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, goneAt(until)).forgotten)
        assertNotNull(p.repo.record.value)
        assertTrue(p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, goneAt(until.plusSeconds(1))).forgotten)
        assertFalse(p.keyStore.hasKey())
    }

    @Test fun `a claim of an earlier version, stored without its time, gets one from the first answer`() = runTest {
        val (p, rec) = phone(KeyDevicePhase.CLAIMING)
        val t = Instant.parse("2026-11-20T10:00:00Z")
        val first = p.answers.settle(rec, origin, Ask.DEVICE_LOGIN_CHALLENGE, ServerAnswer(403, "KEY_DEVICE_REVOKED", serverTime = t))
        assertEquals(KeyAnswers.Settled.Kept(Verdict.KEY_GONE, askAgainIn = Duration.ofMinutes(20)), first)
        val now = p.repo.record.value!!
        assertEquals(t.plus(Duration.ofMinutes(20)), now.claimVoidAfter)
        assertFalse(p.answers.settle(now, origin, Ask.DEVICE_LOGIN_CHALLENGE, ServerAnswer(404, "KEY_DEVICE_UNKNOWN", serverTime = t.plusSeconds(600))).forgotten)
        assertTrue(p.answers.settle(now, origin, Ask.DEVICE_LOGIN_CHALLENGE, ServerAnswer(404, "KEY_DEVICE_UNKNOWN", serverTime = t.plusSeconds(1201))).forgotten)
    }

    private fun KeyDeviceRepository.seedForTest(r: StoredKeyDevice) {
        val f = KeyDeviceRepository::class.java.getDeclaredField("store").apply { isAccessible = true }
        (f.get(this) as InMemoryKeyDeviceStore).stored = r
        reload()
    }
}
