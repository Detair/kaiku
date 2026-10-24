package io.wolftown.kaiku.testutil

import android.content.SharedPreferences
import java.security.GeneralSecurityException

/**
 * [SharedPreferences] that can be told to fail every read the way a damaged
 * EncryptedSharedPreferences does. Everything else goes to [delegate].
 */
class ThrowingReadPreferences(
    private val delegate: SharedPreferences,
    /** Failure thrown by reads while set; checked ones are wrapped by the caller's catch. */
    @Volatile var failure: Throwable? = null,
) : SharedPreferences by delegate {

    private fun <T> read(value: () -> T): T {
        failure?.let { throw it }
        return value()
    }

    override fun getString(key: String?, defValue: String?): String? =
        read { delegate.getString(key, defValue) }

    override fun getLong(key: String?, defValue: Long): Long =
        read { delegate.getLong(key, defValue) }

    companion object {
        fun decryptFailure() = GeneralSecurityException("test")
    }
}
