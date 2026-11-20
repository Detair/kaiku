package io.wolftown.kaiku.data.keydevice

import android.annotation.SuppressLint
import android.content.SharedPreferences
import io.wolftown.kaiku.approval.UvClass
import java.time.Instant

/**
 * Where a key device stands on this phone (lapse is derived from the session, see [KeyDeviceRepository]):
 * [CLAIMING] the claim was sent and its answer is not known yet (kept so a lost answer is reconciled,
 * never guessed); [CLAIMED] a code waits for its confirmation; [PENDING] active at `ready_at`;
 * [ACTIVE] an active key on the server (before or after its first device-key sign-in).
 */
enum class KeyDevicePhase { CLAIMING, CLAIMED, PENDING, ACTIVE }

/**
 * Questions about a record that its server left without an answer, since that server's last answer:
 * when the first one failed ([since]), when the last counted one did ([last]), and on how many
 * separate occasions ([occasions]; see [ServerSilence]). By the phone's own clock.
 */
data class Unanswered(val since: Instant, val last: Instant, val occasions: Int)

/** Everything the phone keeps about its key device; [LocalKeyDevice.claimSecret] is a secret. */
data class StoredKeyDevice(
    val device: LocalKeyDevice,
    val phase: KeyDevicePhase,
    val readyAt: Instant? = null,
    /** C27 `absolute_expires_at` of the session the key last created. */
    val sessionCap: Instant? = null,
    val lastRenewalPromptAt: Instant? = null,
    /** The key this record is about ([io.wolftown.kaiku.approval.KeyId]); null only when the record's key is missing. */
    val keyId: String? = null,
    /** The setup that made this record (null for a record written before this field existed). */
    val attemptId: String? = null,
    /**
     * [KeyDevicePhase.CLAIMING] only: the server time after which the claim can no longer turn this
     * key into a key device (its challenge has run out, and so has any code a claim would have got).
     */
    val claimVoidAfter: Instant? = null,
    /**
     * The install id this key device was enrolled under (the server lists the key device by account
     * and this id). Every question about it is asked with this id, whatever id the app has now;
     * null for a record written before this field existed.
     */
    val installId: String? = null,
    /** The phone's own clock when the claim was recorded. Only [SetupDiscard] reads it. */
    val since: Instant? = null,
    /**
     * The claim of this record can complete only through a code that the phone shows and someone
     * types in elsewhere (a setup link or a scanned code). False for the app's own registration,
     * whose claim completes by itself, and for a record written before this was recorded.
     */
    val codeClaim: Boolean = false,
    /** What [ServerSilence] has noted about this record; null while its server answers. */
    val unanswered: Unanswered? = null,
) {
    /**
     * Whether [other] is this very record: same server, account, key device, enrolment, key, setup
     * and phase. Times kept beside them are not part of what a record is.
     */
    infix fun sameAs(other: StoredKeyDevice?): Boolean = other != null &&
        device.origin == other.device.origin && device.userId == other.device.userId &&
        device.keyDeviceId == other.device.keyDeviceId && device.enrollmentId == other.device.enrollmentId &&
        keyId == other.keyId && attemptId == other.attemptId && phase == other.phase
}

/** What the store holds. "Cannot be read" is not "nothing stored". */
sealed interface KeyStorage {
    data object Absent : KeyStorage
    data class Present(val record: StoredKeyDevice) : KeyStorage
    /** The file exists but cannot be read now (a Keystore failure, a damaged value). */
    data object Unreadable : KeyStorage
}

/** Durable storage of the one record. A write that did not reach storage returns false. */
interface KeyDeviceStore {
    fun load(): KeyStorage
    fun save(s: StoredKeyDevice): Boolean
    fun clear(): Boolean

    /**
     * A flag earlier versions stored while a sign-in made only to remove the phone was running;
     * null when the store cannot be read.
     */
    fun legacyRemovalMark(): Boolean? = false
    fun clearLegacyRemovalMark(): Boolean = true
}

/**
 * [KeyDeviceStore] on Keystore-wrapped preferences (file `kaiku_key_device`).
 * Separate from the session store, so it outlives sign-outs and lapses.
 * Every write is committed, not applied: the caller is told whether it reached storage.
 */
@SuppressLint("ApplySharedPref")
class PrefsKeyDeviceStore(private val prefs: SharedPreferences) : KeyDeviceStore {

    private companion object {
        val logger: java.util.logging.Logger = java.util.logging.Logger.getLogger("KeyDeviceStore")
        const val PHASE = "phase"
        const val KEY_DEVICE_ID = "key_device_id"
        const val USER_ID = "user_id"
        const val ORIGIN = "origin"
        const val UV_CLASS = "uv_class"
        const val ENROLLMENT_ID = "enrollment_id"
        const val CLAIM_SECRET = "claim_secret"
        const val ACCOUNT_NAME = "account_name"
        const val READY_AT = "ready_at"
        const val SESSION_CAP = "session_cap"
        const val RENEWAL_PROMPT = "renewal_prompt_at"
        const val KEY_ID = "key_id"
        const val ATTEMPT_ID = "attempt_id"
        const val CLAIM_VOID_AFTER = "claim_void_after"
        const val INSTALL_ID = "install_id"
        const val SINCE = "since"
        const val CODE_CLAIM = "code_claim"
        const val UNANSWERED_SINCE = "unanswered_since"
        const val UNANSWERED_LAST = "unanswered_last"
        const val UNANSWERED_OCCASIONS = "unanswered_occasions"
        const val LEGACY_REMOVAL_SIGN_IN = "removal_sign_in"
        val REQUIRED = listOf(PHASE, USER_ID, ORIGIN, ENROLLMENT_ID)
    }

    @Synchronized
    override fun load(): KeyStorage = try {
        if (REQUIRED.none(prefs::contains)) {
            KeyStorage.Absent
        } else {
            // A record is there. Anything about it that cannot be read (a failed decrypt, a phase
            // this version does not know, a missing part, an unparsable time) makes it unreadable,
            // never absent: nothing may be decided, written or deleted on a record nobody could read.
            val phase = prefs.getString(PHASE, null)?.let { p -> KeyDevicePhase.entries.firstOrNull { it.name == p } }
            val userId = prefs.getString(USER_ID, null)
            val origin = prefs.getString(ORIGIN, null)
            val enrollmentId = prefs.getString(ENROLLMENT_ID, null)
            if (phase == null || userId == null || origin == null || enrollmentId == null) {
                logger.warning("Key-device store incomplete")
                KeyStorage.Unreadable
            } else {
                KeyStorage.Present(
                    StoredKeyDevice(
                        device = LocalKeyDevice(
                            keyDeviceId = prefs.getString(KEY_DEVICE_ID, null),
                            userId = userId,
                            origin = origin,
                            uvClass = UvClass.entries.firstOrNull { it.wire == prefs.getString(UV_CLASS, null) } ?: UvClass.PER_USE,
                            enrollmentId = enrollmentId,
                            claimSecret = prefs.getString(CLAIM_SECRET, null),
                            accountName = prefs.getString(ACCOUNT_NAME, null),
                        ),
                        phase = phase,
                        readyAt = prefs.getString(READY_AT, null)?.let(Instant::parse),
                        sessionCap = prefs.getString(SESSION_CAP, null)?.let(Instant::parse),
                        lastRenewalPromptAt = prefs.getString(RENEWAL_PROMPT, null)?.let(Instant::parse),
                        keyId = prefs.getString(KEY_ID, null),
                        attemptId = prefs.getString(ATTEMPT_ID, null),
                        claimVoidAfter = prefs.getString(CLAIM_VOID_AFTER, null)?.let(Instant::parse),
                        installId = prefs.getString(INSTALL_ID, null),
                        since = prefs.getString(SINCE, null)?.let(Instant::parse),
                        codeClaim = prefs.getString(CODE_CLAIM, null) == "true",
                        // All three parts or none: a part that is missing or cannot be parsed throws.
                        unanswered = prefs.getString(UNANSWERED_SINCE, null)?.let { since ->
                            Unanswered(
                                Instant.parse(since),
                                Instant.parse(prefs.getString(UNANSWERED_LAST, null)),
                                prefs.getString(UNANSWERED_OCCASIONS, null)!!.toInt(),
                            )
                        },
                    ),
                )
            }
        }
    } catch (e: Exception) {
        // GeneralSecurityException, a Keystore ProviderException, a value of another type, an
        // unparsable time. Nothing is deleted or reset here.
        logger.warning("Key-device store unreadable: ${e.javaClass.simpleName}")
        KeyStorage.Unreadable
    }

    @Synchronized
    override fun save(s: StoredKeyDevice): Boolean = try {
        val d = s.device
        prefs.edit()
            .putString(PHASE, s.phase.name)
            .putString(KEY_DEVICE_ID, d.keyDeviceId)
            .putString(USER_ID, d.userId)
            .putString(ORIGIN, d.origin)
            .putString(UV_CLASS, d.uvClass.wire)
            .putString(ENROLLMENT_ID, d.enrollmentId)
            .putString(CLAIM_SECRET, d.claimSecret)
            .putString(ACCOUNT_NAME, d.accountName)
            .putString(READY_AT, s.readyAt?.toString())
            .putString(SESSION_CAP, s.sessionCap?.toString())
            .putString(RENEWAL_PROMPT, s.lastRenewalPromptAt?.toString())
            .putString(KEY_ID, s.keyId)
            .putString(ATTEMPT_ID, s.attemptId)
            .putString(CLAIM_VOID_AFTER, s.claimVoidAfter?.toString())
            .putString(INSTALL_ID, s.installId)
            .putString(SINCE, s.since?.toString())
            .putString(CODE_CLAIM, if (s.codeClaim) "true" else null)
            .putString(UNANSWERED_SINCE, s.unanswered?.since?.toString())
            .putString(UNANSWERED_LAST, s.unanswered?.last?.toString())
            .putString(UNANSWERED_OCCASIONS, s.unanswered?.occasions?.toString())
            .commit()
    } catch (e: Exception) {
        logger.warning("Key-device store not written: ${e.javaClass.simpleName}")
        false
    }

    @Synchronized
    override fun clear(): Boolean = try {
        // The legacy flag is not part of the record and has its own clean-up.
        val editor = prefs.edit()
        (REQUIRED + listOf(KEY_DEVICE_ID, UV_CLASS, CLAIM_SECRET, ACCOUNT_NAME, READY_AT, SESSION_CAP, RENEWAL_PROMPT, KEY_ID, ATTEMPT_ID, CLAIM_VOID_AFTER, INSTALL_ID, SINCE, CODE_CLAIM, UNANSWERED_SINCE, UNANSWERED_LAST, UNANSWERED_OCCASIONS))
            .forEach { editor.remove(it) }
        editor.commit()
    } catch (e: Exception) {
        logger.warning("Key-device store not cleared: ${e.javaClass.simpleName}")
        false
    }

    @Synchronized
    override fun legacyRemovalMark(): Boolean? = try {
        prefs.contains(LEGACY_REMOVAL_SIGN_IN) && prefs.getBoolean(LEGACY_REMOVAL_SIGN_IN, false)
    } catch (e: Exception) {
        logger.warning("Key-device store unreadable: ${e.javaClass.simpleName}")
        null
    }

    @Synchronized
    override fun clearLegacyRemovalMark(): Boolean = try {
        prefs.edit().remove(LEGACY_REMOVAL_SIGN_IN).commit()
    } catch (e: Exception) {
        false
    }
}
