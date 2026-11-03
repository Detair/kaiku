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
     * Copies [keys] present in [legacy] into [target]. False when [legacy] is
     * null or unreadable, or the write failed: the person signs in again.
     */
    fun migrate(legacy: SharedPreferences?, target: SharedPreferences, keys: List<String>): Boolean {
        if (legacy == null) return false
        val values = try {
            legacy.all
        } catch (e: Exception) {
            logger.warning("Old session store unreadable (${e.javaClass.simpleName}), dropping it")
            return false
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
            editor.commit()
        } catch (e: Exception) {
            logger.warning("Session not moved (${e.javaClass.simpleName}), the person signs in again")
            false
        }
    }

    /**
     * The migration at store open: only when the old store exists; it is opened
     * once ([openLegacy] may throw for a damaged store), copied, and always
     * deleted afterwards. Returns false when the old session was lost. If the
     * new store already holds a session (an earlier move committed but the old
     * file survived, e.g. the app died before the delete), nothing is copied:
     * the old single-use refresh token must never replace a rotated one (C27
     * `SESSION_REUSED`).
     */
    fun run(
        legacyExists: Boolean,
        openLegacy: () -> SharedPreferences?,
        deleteLegacy: () -> Unit,
        target: SharedPreferences,
        keys: List<String>,
    ): Boolean {
        if (!legacyExists) return true
        if (target.contains(ACCESS_TOKEN) || target.contains(REFRESH_TOKEN)) {
            deleteLegacy()
            return true
        }
        val legacy = try {
            openLegacy()
        } catch (e: Exception) {
            logger.warning("Old session store cannot be opened (${e.javaClass.simpleName}), dropping it")
            null
        }
        val ok = migrate(legacy, target, keys)
        deleteLegacy()
        return ok
    }
}
