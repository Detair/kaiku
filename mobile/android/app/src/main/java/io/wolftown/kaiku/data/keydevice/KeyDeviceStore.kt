package io.wolftown.kaiku.data.keydevice

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

/** Everything the phone keeps about its key device; [LocalKeyDevice.claimSecret] is a secret. */
data class StoredKeyDevice(
    val device: LocalKeyDevice,
    val phase: KeyDevicePhase,
    val readyAt: Instant? = null,
    /** C27 `absolute_expires_at` of the session the key last created. */
    val sessionCap: Instant? = null,
    val lastRenewalPromptAt: Instant? = null,
)

interface KeyDeviceStore {
    fun load(): StoredKeyDevice?
    fun save(s: StoredKeyDevice)
    fun clear()
}

/**
 * [KeyDeviceStore] on Keystore-wrapped preferences (file `kaiku_key_device`).
 * Separate from the session store, so it outlives sign-outs and lapses.
 */
class PrefsKeyDeviceStore(private val prefs: SharedPreferences) : KeyDeviceStore {

    private companion object {
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
    }

    @Synchronized
    override fun load(): StoredKeyDevice? = try {
        val phase = prefs.getString(PHASE, null)?.let { p -> KeyDevicePhase.entries.firstOrNull { it.name == p } }
        val userId = prefs.getString(USER_ID, null)
        val origin = prefs.getString(ORIGIN, null)
        val enrollmentId = prefs.getString(ENROLLMENT_ID, null)
        if (phase == null || userId == null || origin == null || enrollmentId == null) {
            null
        } else {
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
            )
        }
    } catch (e: java.security.GeneralSecurityException) {
        // A damaged store reads as "no key device"; the phone is set up again.
        null
    }

    @Synchronized
    override fun save(s: StoredKeyDevice) {
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
            .commit()
    }

    @Synchronized
    override fun clear() {
        prefs.edit().clear().commit()
    }
}
