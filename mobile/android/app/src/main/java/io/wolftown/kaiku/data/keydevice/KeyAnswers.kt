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
    /** The enrolment ended without making a key device, as a state the server has stored (`cancelled`, `failed`). */
    ENROLMENT_ENDED,
    /**
     * The server says the code has run out (`expired`). It works that out from its clock while a
     * confirmation sent just in time may still be being stored, so by itself it forgets nothing:
     * the key-device probe decides, after the time is over by the server's and the phone's measure.
     */
    ENROLMENT_RAN_OUT,
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
 * - an enrolment status counts only as `200` with a stored ending state (`cancelled`, `failed`),
 *   asked for the record's own enrolment (the answer is bound to the id and secret that were
 *   sent); `expired` is worked out from the server's clock and forgets nothing by itself;
 * - "no key device" and "not known" say nothing final about a claim that is not finished: such a
 *   claim is given up on them only once its time is over by the server's and the phone's measure
 *   and a probe asked after that still finds no key device.
 */
class KeyAnswers @Inject constructor(
    private val keyDevices: KeyDeviceRepository,
    /** The phone's running time (how long ago a claim was recorded, see [afterItsTime]). */
    private val boot: BootClock,
) {

    /** One line of the table: [code] for a refusal, [state] for an enrolment status, neither for a plain success. */
    data class Row(val ask: Ask, val status: Int, val code: String?, val state: String?, val verdict: Verdict)

    /**
     * Proof that the claim check said "already claimed" about one record whose own claim never got
     * its answer, and when ([AskOrder]). Only [settle] makes one.
     */
    sealed interface ClaimTaken

    private class TakenFor(val about: StoredKeyDevice, val at: Long) : ClaimTaken

    /**
     * Proof that the time of one unfinished claim is over by both measures: the server's own time
     * (`Date`) is past the claim's time with its margin, and the phone's running time since the
     * claim was recorded is past the longest life of a claim with that margin; and when ([AskOrder]).
     * Only [settle] makes one, from a probe's answer; only a probe sent after it can use it.
     */
    sealed interface TimeOver

    private class OverFor(val about: StoredKeyDevice, val at: Long) : TimeOver

    sealed interface Settled {
        val verdict: Verdict

        /**
         * [askAgainIn]: an unfinished claim may still be stored by its server, or its time has just
         * been found over ([over]); ask the key-device probe again after that long.
         */
        data class Kept(
            override val verdict: Verdict,
            val askAgainIn: Duration? = null,
            /** The claim check said "already claimed"; pass it with the answer of a probe asked afterwards. */
            val taken: ClaimTaken? = null,
            /** The claim's time is over by both measures; pass it with the answer of a probe asked afterwards. */
            val over: TimeOver? = null,
            /** How long until an unfinished claim may be given up, as far as known now (both measures). */
            val left: Duration? = null,
        ) : Settled
        data class Forgotten(override val verdict: Verdict) : Settled

        val forgotten: Boolean get() = this is Forgotten
    }

    companion object {
        /** After a claim's challenge ran out: the life of a code (5 min) and a margin (5 min). */
        val CLAIM_VOID_MARGIN: Duration = Duration.ofMinutes(10)

        /** For a claim whose challenge time is not stored: a challenge's longest life (10 min) on top. */
        val CLAIM_VOID_UNKNOWN: Duration = Duration.ofMinutes(20)

        /**
         * The longest life of a claim (its challenge, 10 min, then its code, 5 min) and the margin
         * (10 min): by the phone's running time since the claim was recorded.
         */
        val CLAIM_LIFE_WITH_MARGIN: Duration = Duration.ofMinutes(25)

        /**
         * For a claim whose code arrived and whose time the server stated: the margin after the
         * code's expiry, on the server's clock and on the phone's running time. It covers a
         * confirmation that passed the server's expiry check just in time and is stored late.
         */
        val CODE_MARGIN: Duration = Duration.ofMinutes(2)

        /**
         * How long a code lives on the server (`CODE_MINUTES` in the server's enrolments). The
         * phone waits at least this long after a code arrived, whatever the answer's `Date` says
         * (a clock in front of the server may run ahead); a longer lifetime on the server is still
         * honoured through the `Date` it sends with the code.
         */
        val SERVER_CODE_LIFETIME: Duration = Duration.ofMinutes(5)

        /**
         * The phone's running time to wait after a code arrived: the code's remaining life (at
         * least [SERVER_CODE_LIFETIME], or more if the server's `Date` says so) and [CODE_MARGIN].
         */
        fun codeWait(serverTime: Instant, codeExpires: Instant): Duration =
            maxOf(SERVER_CODE_LIFETIME, Duration.between(serverTime, codeExpires)).plus(CODE_MARGIN)

        /** While only the phone's own time is left to pass, the probe is asked again at least this often. */
        val ASK_AGAIN_WHILE_WAITING: Duration = Duration.ofMinutes(5)

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
            Row(Ask.ENROLMENT_STATUS, 200, null, "expired", Verdict.ENROLMENT_RAN_OUT),
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
     * - [Verdict.KEY_GONE]: an active or pending key device (not one whose install id could not be
     *   established); an unfinished claim only through the probe's own form of this function (below);
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
    ): Settled = decide(expected, askedOrigin, ask, answer, askedEnrolment, null, null, null)

    /**
     * The key-device probe said "no key device" about [expected]: [gone] is the probe's own result
     * and carries when its question was sent. Only here can an unfinished claim be given up, in one
     * of two ways, each only for a probe sent after its proof was made:
     * - [taken]: the claim's answer never came (`CLAIMING`), and the claim check said "already
     *   claimed" about that very record;
     * - [over]: the claim's time is over by the server's and the phone's measure ([afterItsTime]).
     */
    fun settle(
        expected: StoredKeyDevice,
        askedOrigin: String?,
        gone: ServerKeyState.Gone,
        taken: ClaimTaken? = null,
        over: TimeOver? = null,
    ): Settled = decide(expected, askedOrigin, Ask.DEVICE_LOGIN_CHALLENGE, gone.answer, null, taken, over, gone.sent)

    private fun decide(
        expected: StoredKeyDevice,
        askedOrigin: String?,
        ask: Ask,
        answer: ServerAnswer,
        askedEnrolment: String?,
        taken: ClaimTaken?,
        over: TimeOver?,
        sent: ProbeSent?,
    ): Settled {
        val own = KaikuPayloads.canonicalOrigin(expected.device.origin)
        if (askedOrigin == null || own == null || askedOrigin != own || !own.startsWith("https://")) return Settled.Kept(Verdict.KEEP)
        val v = verdict(ask, answer)
        // A record that took the server's word about this install only provisionally is still the
        // claim it was: no answer forgets it as a key device.
        val phase = expected.provisionalFrom ?: expected.phase
        val unfinished = phase == KeyDevicePhase.CLAIMING || phase == KeyDevicePhase.CLAIMED
        // The claim's answer never came (the stored phase says so), its enrolment is claimed all
        // the same, and a probe asked after that answer finds no key device.
        val claimLost = phase == KeyDevicePhase.CLAIMING && ask == Ask.DEVICE_LOGIN_CHALLENGE &&
            taken is TakenFor && taken.about sameAs expected && sent != null && sent.isAfter(taken.at)
        return when (v) {
            Verdict.KEY_GONE -> when {
                // "No key device for this install" says nothing about a key whose install id is not
                // known: lost, or never recorded (an id the app has now may have been made later).
                expected.installIdLost || expected.installId == null -> Settled.Kept(v)
                phase == KeyDevicePhase.ACTIVE || phase == KeyDevicePhase.PENDING || claimLost -> drop(expected, v)
                unfinished && sent != null -> afterItsTime(expected, answer.serverTime, v, over, sent)
                else -> Settled.Kept(v)
            }
            Verdict.REMOVED -> if (phase == KeyDevicePhase.ACTIVE) drop(expected, v) else Settled.Kept(v)
            Verdict.CLAIM_REFUSED, Verdict.CLAIM_VOID -> if (phase == KeyDevicePhase.CLAIMING) drop(expected, v) else Settled.Kept(v)
            Verdict.ENROLMENT_ENDED ->
                if (phase == KeyDevicePhase.CLAIMED && askedEnrolment == expected.device.enrollmentId) drop(expected, v) else Settled.Kept(v)
            Verdict.CLAIM_TAKEN -> Settled.Kept(
                v,
                taken = if (ask == Ask.CLAIM_CHECK && phase == KeyDevicePhase.CLAIMING) TakenFor(expected, AskOrder.next()) else null,
            )
            Verdict.KEEP, Verdict.CLAIM_UNDECIDED, Verdict.ENROLMENT_UNKNOWN, Verdict.ENROLMENT_RAN_OUT -> Settled.Kept(v)
        }
    }

    /**
     * An unfinished claim (`CLAIMING`, or `CLAIMED` whose code ran out) about which only the
     * key-device probe is left to ask: "no key device" forgets it only when all three hold:
     * 1. the server's own time ([serverTime], its `Date`) is past the claim's time with its margin
     *    ([StoredKeyDevice.claimVoidAfter]: for a code that arrived with its time, the code's
     *    expiry plus [CODE_MARGIN]; otherwise the challenge's expiry plus [CLAIM_VOID_MARGIN]);
     * 2. the phone's running time is past the same span: since the code arrived, the code's
     *    remaining life then (at least [SERVER_CODE_LIFETIME]) plus [CODE_MARGIN]
     *    ([StoredKeyDevice.codeWait], [codeWait]); otherwise, since the
     *    claim was recorded, [CLAIM_LIFE_WITH_MARGIN];
     * 3. this probe was sent after 1 and 2 were found ([over], made by an earlier call here).
     * Until then the key stays and the probe is asked again. A record without its time gets one
     * from this answer, one without its running time gets it now; both count from there.
     */
    private fun afterItsTime(expected: StoredKeyDevice, serverTime: Instant?, v: Verdict, over: TimeOver?, sent: ProbeSent): Settled {
        if (over is OverFor && over.about sameAs expected && sent.isAfter(over.at)) return drop(expected, v)
        val now = serverTime ?: return Settled.Kept(v)
        val until = expected.claimVoidAfter
        if (until == null) {
            // Only a time that reached storage counts; otherwise the next answer sets it.
            val noted = keyDevices.update(expected) { it.copy(claimVoidAfter = now.plus(CLAIM_VOID_UNKNOWN)) }.done
            return Settled.Kept(v, askAgainIn = if (noted) CLAIM_VOID_UNKNOWN else null)
        }
        val serverLeft = if (now.isAfter(until)) Duration.ZERO else Duration.between(now, until)
        val ownLeft = ownTimeLeft(expected) ?: return Settled.Kept(v)
        if (serverLeft.isZero && ownLeft.isZero) {
            return Settled.Kept(v, askAgainIn = Duration.ZERO, over = OverFor(expected, AskOrder.next()), left = Duration.ZERO)
        }
        // Asked again when the server's time is over, and then every few minutes while the
        // phone's own time runs: a claim that completed late is taken over as soon as it shows.
        return Settled.Kept(
            v,
            askAgainIn = if (!serverLeft.isZero) serverLeft else minOf(ownLeft, ASK_AGAIN_WHILE_WAITING),
            left = maxOf(serverLeft, ownLeft),
        )
    }

    /**
     * How much of [CLAIM_LIFE_WITH_MARGIN] is still left, by the phone's running time since the
     * claim was recorded; zero once it has passed. A record that does not say when it was written
     * is noted now and has the whole span left (null when the note did not reach storage).
     */
    private fun ownTimeLeft(expected: StoredKeyDevice): Duration? {
        codeTimeLeft(expected)?.let { return it }
        val written = expected.written
            ?: return if (keyDevices.update(expected) { it.copy(written = boot.now()) }.done) CLAIM_LIFE_WITH_MARGIN else null
        val passed = boot.since(written)
        return if (passed >= CLAIM_LIFE_WITH_MARGIN) Duration.ZERO else CLAIM_LIFE_WITH_MARGIN.minus(passed)
    }

    /**
     * A claim whose code arrived with its time: how much of [StoredKeyDevice.codeWait] is still
     * left by the phone's running time since the code arrived; null for any other record.
     */
    private fun codeTimeLeft(expected: StoredKeyDevice): Duration? {
        if (expected.phase != KeyDevicePhase.CLAIMED) return null
        val received = expected.codeReceived ?: return null
        // A wait stored before it had its floor gets the floor a new one gets.
        val wait = maxOf(expected.codeWait ?: return null, SERVER_CODE_LIFETIME.plus(CODE_MARGIN))
        val passed = boot.since(received)
        return if (passed >= wait) Duration.ZERO else wait.minus(passed)
    }

    /**
     * About how long until an unfinished claim may be given up, by the phone's running time alone
     * (the server's time is known only from an answer); null when nothing is known yet.
     */
    fun ownWaitLeft(record: StoredKeyDevice): Duration? = codeTimeLeft(record)
        ?: record.written?.let { w -> CLAIM_LIFE_WITH_MARGIN.minus(boot.since(w)).coerceAtLeast(Duration.ZERO) }

    private fun drop(expected: StoredKeyDevice, v: Verdict): Settled =
        if (keyDevices.forget(expected, Forget.SERVER_ANSWER).done) Settled.Forgotten(v) else Settled.Kept(v)
}
