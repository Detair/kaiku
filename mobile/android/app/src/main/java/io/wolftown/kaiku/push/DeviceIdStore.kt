package io.wolftown.kaiku.push

import android.content.Context
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.approval.DeviceIdProvider
import java.util.UUID
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The push device id (C19): a random UUID created on first use and kept for
 * the life of the installation. It is not the E2EE device id and is not
 * cleared on logout, so the next account on this phone replaces the same
 * server row. Not a secret.
 */
@Singleton
class DeviceIdStore @Inject constructor(
    @ApplicationContext private val context: Context,
) : DeviceIdProvider {
    private companion object {
        const val PREFS = "push_prefs"
        const val KEY_DEVICE_ID = "push_device_id"
    }

    private val lock = Any()

    @Volatile
    private var cached: String? = null

    /** Also the C26 `device_id` of this install's key device, so C28 pushes reach it. */
    override fun id(): String {
        cached?.let { return it }
        return synchronized(lock) {
            cached ?: run {
                val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                val stored = prefs.getString(KEY_DEVICE_ID, null)?.takeIf(::isUuid)
                val id = stored ?: UUID.randomUUID().toString().also {
                    // Synchronous, so no second pass (or process) can mint another id.
                    check(prefs.edit().putString(KEY_DEVICE_ID, it).commit()) { "push device id not stored" }
                }
                cached = id
                id
            }
        }
    }

    private fun isUuid(value: String): Boolean =
        try {
            UUID.fromString(value).toString() == value.lowercase()
        } catch (_: IllegalArgumentException) {
            false
        }
}
