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

    /** Reads that still fail; negative means every read fails while [failure] is set. */
    @Volatile var failures: Int = -1

    /** Runs when a read is about to fail, e.g. to simulate a writer committing meanwhile. */
    @Volatile var onFailure: (() -> Unit)? = null

    private fun <T> read(value: () -> T): T {
        val f = failure
        if (f != null && failures != 0) {
            if (failures > 0) failures--
            onFailure?.invoke()
            throw f
        }
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
