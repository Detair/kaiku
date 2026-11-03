package io.wolftown.kaiku.data.local

import android.content.SharedPreferences
import java.util.logging.Logger

/**
 * One-time move of the session from the old `security-crypto` store
 * (`kaiku_secure_prefs`) into the Keystore-wrapped store, so existing sign-ins
 * stay valid (spec F8 migration; coordinator call 4: `security-crypto` stays
 * one release for this read only).
 */
object LegacyTokenMigration {
    private val logger = Logger.getLogger("LegacyTokenMigration")

    /** [TokenStorage]'s session keys: their presence means a session was moved already. */
    private const val ACCESS_TOKEN = "access_token"
    private const val REFRESH_TOKEN = "refresh_token"

    /**
     * Ends a pending move for good (Codex r3): once this phone has a new session or signed
     * out, the old store's session must never be moved in later. Deletes the old store and
     * the retry counter; nothing to do when no move is pending.
     */
    fun discardPending(legacyExists: Boolean, deleteLegacy: () -> Unit, failedWrites: Int, recordFailedWrites: (Int) -> Unit) {
        if (!legacyExists && failedWrites == 0) return
        deleteLegacy()
        recordFailedWrites(0)
    }

    /** Starts with a failed write before the old store is given up (Fable m7). */
    const val MAX_WRITE_ATTEMPTS = 3

    /** How one move went. */
    enum class Outcome {
        MOVED,
        /** The old store is null or unreadable: nothing to keep. */
        LEGACY_UNREADABLE,
        /** The old store was read but the new store could not be written (Keystore trouble). */
        WRITE_FAILED,
    }

    /**
     * Copies [keys] present in [legacy] into [target]. False when [legacy] is
     * null or unreadable, or the write failed: the person signs in again.
     */
    fun migrate(legacy: SharedPreferences?, target: SharedPreferences, keys: List<String>): Boolean =
        move(legacy, target, keys) == Outcome.MOVED

    private fun move(legacy: SharedPreferences?, target: SharedPreferences, keys: List<String>): Outcome {
        if (legacy == null) return Outcome.LEGACY_UNREADABLE
        val values = try {
            legacy.all
        } catch (e: Exception) {
            logger.warning("Old session store unreadable (${e.javaClass.simpleName}), dropping it")
            return Outcome.LEGACY_UNREADABLE
        }
        // Encrypting goes through the Android Keystore, which can fail (ProviderException,
        // KeyStoreException): the person then signs in again; the app still starts.
        return try {
            val editor = target.edit()
            for (key in keys) {
                when (val v = values[key]) {
                    is String -> editor.putString(key, v)
                    is Long -> editor.putLong(key, v)
                    is Int -> editor.putInt(key, v)
                    is Boolean -> editor.putBoolean(key, v)
                    is Float -> editor.putFloat(key, v)
                    else -> Unit
                }
            }
            if (editor.commit()) Outcome.MOVED else Outcome.WRITE_FAILED
        } catch (e: Exception) {
            logger.warning("Session not moved (${e.javaClass.simpleName})")
            Outcome.WRITE_FAILED
        }
    }

    /**
     * The migration at store open: only when the old store exists; it is opened
     * once ([openLegacy] may throw for a damaged store), copied, and always
     * deleted afterwards. Returns false when the old session was lost. If the
     * new store already holds a session (an earlier move committed but the old
     * file survived, e.g. the app died before the delete), nothing is copied:
     * the old single-use refresh token must never replace a rotated one (C27
     * `SESSION_REUSED`). A failed write keeps the old store for the next start, up
     * to [MAX_WRITE_ATTEMPTS] starts ([failedWrites] / [recordFailedWrites], kept
     * outside both stores); only a copied or unreadable old store is deleted at once.
     */
    fun run(
        legacyExists: Boolean,
        openLegacy: () -> SharedPreferences?,
        deleteLegacy: () -> Unit,
        target: SharedPreferences,
        keys: List<String>,
        failedWrites: Int = 0,
        recordFailedWrites: (Int) -> Unit = {},
    ): Boolean {
        if (!legacyExists) return true
        if (target.contains(ACCESS_TOKEN) || target.contains(REFRESH_TOKEN)) {
            deleteLegacy()
            recordFailedWrites(0)
            return true
        }
        val legacy = try {
            openLegacy()
        } catch (e: Exception) {
            logger.warning("Old session store cannot be opened (${e.javaClass.simpleName}), dropping it")
            null
        }
        val outcome = move(legacy, target, keys)
        if (outcome == Outcome.WRITE_FAILED && failedWrites + 1 < MAX_WRITE_ATTEMPTS) {
            // A passing Keystore failure: the session is tried again at the next start.
            recordFailedWrites(failedWrites + 1)
            return false
        }
        deleteLegacy()
        recordFailedWrites(0)
        return outcome == Outcome.MOVED
    }
}
