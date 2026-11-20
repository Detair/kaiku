package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.domain.KaikuPayloads
import java.time.Duration
import java.time.Instant
import javax.inject.Inject

/** The requests whose answer can say something about this phone's key. */
enum class Ask {
    /** `POST /auth/device-login/challenge` (also the probe that leaves the challenge unanswered). */
    DEVICE_LOGIN_CHALLENGE,
    /** `POST /auth/device-login`. */
    DEVICE_LOGIN,
    /** `POST /api/approvals/step-up` and the answer to its challenge. */
    STEP_UP,
    /** `DELETE /api/approval-devices/{id}` for this phone. */
    KEY_DEVICE_REMOVE,
    /** `POST /auth/enrollments/claim`. */
    CLAIM,
    /** `POST /auth/enrollments/pop-challenge` for the link and key of a claim whose answer is not known. */
    CLAIM_CHECK,
    /** `POST /auth/enrollments/status`. */
    ENROLMENT_STATUS,
}

/** What an answer means for the record it was asked about. */
enum class Verdict {
    /** Nothing is known that could cost the key. Every answer that is not listed. */
    KEEP,
    /** The server knows no key device for this account on this phone, or knows it as removed. */
    KEY_GONE,
    /** The server removed this phone as a key because its owner asked for it. */
    REMOVED,
    /** This claim was refused for good and nothing was stored for its key. */
    CLAIM_REFUSED,
    /** The claim may have been stored (possibly by this very request): the claim check decides. */
    CLAIM_UNDECIDED,
    /** The enrolment is still open and every earlier claim of it is void: nothing was stored for the key. */
    CLAIM_VOID,
    /**
     * The enrolment is claimed. For a claim whose answer this phone never got, the key-device probe
     * asked after this answer says whether that claim made this phone a key.
     */
    CLAIM_TAKEN,
    /** The enrolment ended without making a key device. */
    ENROLMENT_ENDED,
    /** The server does not know the enrolment (any more); whether a key device came of it, the key-device probe says. */
    ENROLMENT_UNKNOWN,
}

/**
 * A server answer as far as it matters for a key: the HTTP status, the error code of a refusal or
 * the `state` of an enrolment status, and the server's own time (`Date`) when it sent one.
 */
data class ServerAnswer(val status: Int, val code: String? = null, val state: String? = null, val serverTime: Instant? = null) {
    companion object {
        fun of(e: ApiException): ServerAnswer = ServerAnswer(e.status.value, e.normalizedCode, serverTime = e.fields.serverTime)
    }
}

/**
 * The order in which questions were asked and answers taken, in this process: a number that only
 * grows. One answer can then be required to belong to a question asked after another answer. A
 * number from here is not a stamp by itself: the probe's is a [ProbeSent], the claim check's is
 * inside its proof.
 */
internal object AskOrder {
    private val n = java.util.concurrent.atomic.AtomicLong()
    fun next(): Long = n.incrementAndGet()
}

/**
 * Every answer that can cost this phone its key, as data ([rows]), and the one function through
 * which such an answer forgets a record ([settle]). An answer that is not in the table (another
 * status with a listed code, an unknown code, no readable body, any status not listed) keeps the key.
 *
 * Why these rows (server: `approvals/enrollments`, `approvals/device_login`, `approvals/devices`):
 * - the two key-device answers are the server's own for "no such key device" and "removed";
 * - the refused claims are decided either before anything is read (the request's shape) or while
 *   the server holds the enrolment's lock with the enrolment still open, and come out the same
 *   for the same request whenever it is handled, so nothing was or will be stored for the key;
 * - "already claimed" can be this phone's own claim that was stored after its answer was lost,
 *   and "not valid" is first read without the lock: neither forgets; the claim check asks under
 *   the lock (a new challenge for the same link and key voids every earlier one);
 * - a claim check that says "already claimed" (under the lock: a claim is stored) for a record
 *   whose own claim never got its answer, followed by a key-device probe asked after it that says
 *   "no key device": the stored claim is not a key device of this phone and cannot become one
 *   (a code it may have got was in the answer that never arrived, so nobody can confirm it; or
 *   another phone holds the link). In that order a claim that completed by itself shows as a key
 *   device in the probe and is taken over instead;
 * - an enrolment status counts only as `200` with an ending state, asked for the record's own
 *   enrolment (the answer is bound to the id and secret that were sent).
 */
class KeyAnswers internal constructor(
    private val keyDevices: KeyDeviceRepository,
    /** The phone's own clock (only for how long ago a claim was recorded, see [afterItsTime]). */
    private val clock: () -> Instant,
) {

    @Inject
    constructor(keyDevices: KeyDeviceRepository) : this(keyDevices, Instant::now)

    /** One line of the table: [code] for a refusal, [state] for an enrolment status, neither for a plain success. */
    data class Row(val ask: Ask, val status: Int, val code: String?, val state: String?, val verdict: Verdict)

    /**
     * Proof that the server no longer knows the enrolment of one record. Only [settle] makes one
     * (the one implementation is private to this class), and it counts only for that very record.
     */
    sealed interface EnrolmentUnknown

    private class UnknownFor(val about: StoredKeyDevice) : EnrolmentUnknown

    /**
     * Proof that the claim check said "already claimed" about one record whose own claim never got
     * its answer, and when ([AskOrder]). Only [settle] makes one.
     */
    sealed interface ClaimTaken

    private class TakenFor(val about: StoredKeyDevice, val at: Long) : ClaimTaken

    sealed interface Settled {
        val verdict: Verdict

        /**
         * [unknown]: the enrolment is not known to the server; pass it with the key-device probe's
         * answer. [askAgainIn]: a claim without an answer is still within the time in which the
         * server could store it; ask again after that long.
         */
        data class Kept(
            override val verdict: Verdict,
            val unknown: EnrolmentUnknown? = null,
            val askAgainIn: Duration? = null,
            /** The claim check said "already claimed"; pass it with the answer of a probe asked afterwards. */
            val taken: ClaimTaken? = null,
        ) : Settled
        data class Forgotten(override val verdict: Verdict) : Settled

        val forgotten: Boolean get() = this is Forgotten
    }

    companion object {
        /** After a claim's challenge ran out: the life of a code (5 min) and a margin (5 min). */
        val CLAIM_VOID_MARGIN: Duration = Duration.ofMinutes(10)

        /** For a claim whose challenge time is not stored: a challenge's longest life (10 min) on top. */
        val CLAIM_VOID_UNKNOWN: Duration = Duration.ofMinutes(20)

        private fun gone(ask: Ask) = listOf(
            Row(ask, 404, "KEY_DEVICE_UNKNOWN", null, Verdict.KEY_GONE),
            Row(ask, 403, "KEY_DEVICE_REVOKED", null, Verdict.KEY_GONE),
        )

        val rows: List<Row> = gone(Ask.DEVICE_LOGIN_CHALLENGE) + gone(Ask.DEVICE_LOGIN) + gone(Ask.KEY_DEVICE_REMOVE) + listOf(
            Row(Ask.KEY_DEVICE_REMOVE, 200, null, null, Verdict.REMOVED),
            Row(Ask.CLAIM, 400, "VALIDATION_ERROR", null, Verdict.CLAIM_REFUSED),
            Row(Ask.CLAIM, 400, "SIGNATURE_INVALID", null, Verdict.CLAIM_REFUSED),
            Row(Ask.CLAIM, 400, "DEVICE_UNSUITABLE", null, Verdict.CLAIM_REFUSED),
            Row(Ask.CLAIM, 404, "APPROVAL_NOT_FOUND", null, Verdict.CLAIM_REFUSED),
            Row(Ask.CLAIM, 410, "APPROVAL_EXPIRED", null, Verdict.CLAIM_REFUSED),
            Row(Ask.CLAIM, 409, "ENROLLMENT_ALREADY_CLAIMED", null, Verdict.CLAIM_UNDECIDED),
            Row(Ask.CLAIM, 404, "ENROLLMENT_TOKEN_INVALID", null, Verdict.CLAIM_UNDECIDED),
            Row(Ask.CLAIM, 409, "APPROVAL_STATE", null, Verdict.CLAIM_UNDECIDED),
            Row(Ask.CLAIM, 409, "ENROLLMENT_STATE", null, Verdict.CLAIM_UNDECIDED),
            Row(Ask.CLAIM_CHECK, 201, null, null, Verdict.CLAIM_VOID),
            Row(Ask.CLAIM_CHECK, 409, "ENROLLMENT_ALREADY_CLAIMED", null, Verdict.CLAIM_TAKEN),
            Row(Ask.CLAIM_CHECK, 404, "ENROLLMENT_TOKEN_INVALID", null, Verdict.ENROLMENT_UNKNOWN),
            Row(Ask.ENROLMENT_STATUS, 200, null, "cancelled", Verdict.ENROLMENT_ENDED),
            Row(Ask.ENROLMENT_STATUS, 200, null, "expired", Verdict.ENROLMENT_ENDED),
            Row(Ask.ENROLMENT_STATUS, 200, null, "failed", Verdict.ENROLMENT_ENDED),
            Row(Ask.ENROLMENT_STATUS, 404, "ENROLLMENT_TOKEN_INVALID", null, Verdict.ENROLMENT_UNKNOWN),
        )

        /** The table alone: what [answer] to [ask] means. */
        fun verdict(ask: Ask, answer: ServerAnswer): Verdict =
            rows.firstOrNull { it.ask == ask && it.status == answer.status && it.code == answer.code && it.state == answer.state }?.verdict
                ?: Verdict.KEEP
    }

    /**
     * The only place a server answer forgets a record and its key.
     *
     * [expected] is the record the request was about, [askedOrigin] the origin the request went
     * to: it must be the record's own server over https, or the answer counts for nothing.
     * [askedEnrolment] is the enrolment id an [Ask.ENROLMENT_STATUS] was sent for. The verdict
     * comes from [rows]; it forgets only a record in the phase it can speak about, and only if that
     * record is still the stored one ([KeyDeviceRepository.forget]):
     * - [Verdict.KEY_GONE]: an active or pending key device. A claim (`CLAIMING`, `CLAIMED`) only
     *   with [unknown] (the server no longer knows its enrolment); a claim whose answer never came
     *   (`CLAIMING`) also through the probe's own form of this function (below), or once the
     *   server's own clock is past the time the claim could still be stored;
     * - [Verdict.REMOVED]: an active key device;
     * - [Verdict.CLAIM_REFUSED], [Verdict.CLAIM_VOID]: a claim whose answer was not known (`CLAIMING`);
     * - [Verdict.ENROLMENT_ENDED]: a claim waiting for its code (`CLAIMED`) of that very enrolment.
     */
    fun settle(
        expected: StoredKeyDevice,
        askedOrigin: String?,
        ask: Ask,
        answer: ServerAnswer,
        askedEnrolment: String? = null,
        unknown: EnrolmentUnknown? = null,
    ): Settled = decide(expected, askedOrigin, ask, answer, askedEnrolment, unknown, null, null)

    /**
     * The key-device probe said "no key device" about [expected]: [gone] is the probe's own result
     * and carries when its question was sent. Only here can [taken] count: for a claim whose
     * answer never came (`CLAIMING`), when the claim check said "already claimed" about that very
     * record and this probe was sent after that answer.
     */
    fun settle(
        expected: StoredKeyDevice,
        askedOrigin: String?,
        gone: ServerKeyState.Gone,
        unknown: EnrolmentUnknown? = null,
        taken: ClaimTaken? = null,
    ): Settled = decide(expected, askedOrigin, Ask.DEVICE_LOGIN_CHALLENGE, gone.answer, null, unknown, taken, gone.sent)

    private fun decide(
        expected: StoredKeyDevice,
        askedOrigin: String?,
        ask: Ask,
        answer: ServerAnswer,
        askedEnrolment: String?,
        unknown: EnrolmentUnknown?,
        taken: ClaimTaken?,
        sent: ProbeSent?,
    ): Settled {
        val own = KaikuPayloads.canonicalOrigin(expected.device.origin)
        if (askedOrigin == null || own == null || askedOrigin != own || !own.startsWith("https://")) return Settled.Kept(Verdict.KEEP)
        val v = verdict(ask, answer)
        val proven = unknown is UnknownFor && unknown.about sameAs expected
        val phase = expected.phase
        // The claim's answer never came (the stored phase says so), its enrolment is claimed all
        // the same, and a probe asked after that answer finds no key device.
        val claimLost = phase == KeyDevicePhase.CLAIMING && ask == Ask.DEVICE_LOGIN_CHALLENGE &&
            taken is TakenFor && taken.about sameAs expected && sent != null && sent.isAfter(taken.at)
        return when (v) {
            Verdict.KEY_GONE -> when {
                phase == KeyDevicePhase.ACTIVE || phase == KeyDevicePhase.PENDING || proven || claimLost -> drop(expected, v)
                // A claim that has no claim secret to ask its enrolment with.
                phase == KeyDevicePhase.CLAIMING || expected.device.claimSecret == null -> afterItsTime(expected, answer.serverTime, v)
                else -> Settled.Kept(v)
            }
            Verdict.REMOVED -> if (phase == KeyDevicePhase.ACTIVE) drop(expected, v) else Settled.Kept(v)
            Verdict.CLAIM_REFUSED, Verdict.CLAIM_VOID -> if (phase == KeyDevicePhase.CLAIMING) drop(expected, v) else Settled.Kept(v)
            Verdict.ENROLMENT_ENDED ->
                if (phase == KeyDevicePhase.CLAIMED && askedEnrolment == expected.device.enrollmentId) drop(expected, v) else Settled.Kept(v)
            Verdict.ENROLMENT_UNKNOWN -> {
                val about = (ask == Ask.CLAIM_CHECK && phase == KeyDevicePhase.CLAIMING) ||
                    (ask == Ask.ENROLMENT_STATUS && phase == KeyDevicePhase.CLAIMED && askedEnrolment == expected.device.enrollmentId)
                Settled.Kept(v, unknown = if (about) UnknownFor(expected) else null)
            }
            Verdict.CLAIM_TAKEN -> Settled.Kept(
                v,
                taken = if (ask == Ask.CLAIM_CHECK && phase == KeyDevicePhase.CLAIMING) TakenFor(expected, AskOrder.next()) else null,
            )
            Verdict.KEEP, Verdict.CLAIM_UNDECIDED -> Settled.Kept(v)
        }
    }

    /**
     * A claim whose answer never came, with only the key-device probe left to ask (the link is
     * gone with the process): "no key device" forgets it once nothing can turn it into one any
     * more, by the server's own clock. A record without that time gets one from this answer.
     *
     * A claim that completes by itself (the app's own registration: no code) could still be
     * stored by its server just after a restart, whatever time the answer's `Date` names (a proxy
     * in front of the server may set it): for such a claim the phone's own clock must also say
     * that the claim's longest life has passed since it was recorded ([ownTimeLeft]).
     */
    private fun afterItsTime(expected: StoredKeyDevice, serverTime: Instant?, v: Verdict): Settled {
        val now = serverTime ?: return Settled.Kept(v)
        val until = expected.claimVoidAfter
        if (until == null) {
            // Only a time that reached storage counts; otherwise the next answer sets it.
            val noted = keyDevices.update(expected) { it.copy(claimVoidAfter = now.plus(CLAIM_VOID_UNKNOWN)) }.done
            return Settled.Kept(v, askAgainIn = if (noted) CLAIM_VOID_UNKNOWN else null)
        }
        if (!now.isAfter(until)) return Settled.Kept(v, askAgainIn = Duration.between(now, until))
        if (expected.phase == KeyDevicePhase.CLAIMING && !expected.codeClaim) {
            val left = ownTimeLeft(expected) ?: return Settled.Kept(v)
            if (!left.isZero) return Settled.Kept(v, askAgainIn = left)
        }
        return drop(expected, v)
    }

    /**
     * How long, by the phone's own clock, until [CLAIM_VOID_UNKNOWN] (a challenge's longest life
     * and the margin) has passed since the claim was recorded; zero once it has. A record without
     * that time, or with one the clock has not reached yet (it was set back), counts from now: the
     * time is noted and the whole span is left (null when the note did not reach storage).
     */
    private fun ownTimeLeft(expected: StoredKeyDevice): Duration? {
        val phoneNow = clock()
        val since = expected.since
        if (since == null || since.isAfter(phoneNow)) {
            return if (keyDevices.update(expected) { it.copy(since = phoneNow) }.done) CLAIM_VOID_UNKNOWN else null
        }
        val passed = Duration.between(since, phoneNow)
        return if (passed >= CLAIM_VOID_UNKNOWN) Duration.ZERO else CLAIM_VOID_UNKNOWN.minus(passed)
    }

    private fun drop(expected: StoredKeyDevice, v: Verdict): Settled =
        if (keyDevices.forget(expected, Forget.SERVER_ANSWER).done) Settled.Forgotten(v) else Settled.Kept(v)
}
