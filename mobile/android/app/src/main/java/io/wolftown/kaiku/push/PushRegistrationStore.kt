package io.wolftown.kaiku.push

import android.content.SharedPreferences
import java.security.MessageDigest

/**
 * The last registration the server accepted for this installation. Holds a
 * SHA-256 of the endpoint, never the endpoint itself.
 */
data class PushRegistrationRecord(
    val userId: String,
    val serverUrl: String?,
    val provider: String,
    val endpointHash: String,
    val registeredAtMs: Long,
) {
    companion object {
        fun hash(endpoint: String): String =
            MessageDigest.getInstance("SHA-256")
                .digest(endpoint.toByteArray(Charsets.UTF_8))
                .joinToString("") { "%02x".format(it) }
    }
}

interface PushRegistrationStore {
    fun load(): PushRegistrationRecord?
    fun save(record: PushRegistrationRecord)
    fun clear()
}

class PrefsPushRegistrationStore(private val prefs: SharedPreferences) : PushRegistrationStore {

    private companion object {
        const val KEY_USER = "reg_user_id"
        const val KEY_SERVER = "reg_server_url"
        const val KEY_PROVIDER = "reg_provider"
        const val KEY_HASH = "reg_endpoint_hash"
        const val KEY_AT = "reg_registered_at"
    }

    override fun load(): PushRegistrationRecord? {
        val userId = prefs.getString(KEY_USER, null) ?: return null
        val provider = prefs.getString(KEY_PROVIDER, null) ?: return null
        val hash = prefs.getString(KEY_HASH, null) ?: return null
        return PushRegistrationRecord(
            userId = userId,
            serverUrl = prefs.getString(KEY_SERVER, null),
            provider = provider,
            endpointHash = hash,
            registeredAtMs = prefs.getLong(KEY_AT, 0L),
        )
    }

    override fun save(record: PushRegistrationRecord) {
        prefs.edit()
            .putString(KEY_USER, record.userId)
            .putString(KEY_SERVER, record.serverUrl)
            .putString(KEY_PROVIDER, record.provider)
            .putString(KEY_HASH, record.endpointHash)
            .putLong(KEY_AT, record.registeredAtMs)
            .apply()
    }

    override fun clear() {
        prefs.edit()
            .remove(KEY_USER)
            .remove(KEY_SERVER)
            .remove(KEY_PROVIDER)
            .remove(KEY_HASH)
            .remove(KEY_AT)
            .apply()
    }
}
