package io.wolftown.kaiku.data.local

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Authenticated encryption of one stored value; [aad] binds it to its key name. */
interface ValueCipher {
    fun encrypt(plain: ByteArray, aad: ByteArray): ByteArray

    /** Throws [GeneralSecurityException] for a damaged or moved blob. */
    fun decrypt(blob: ByteArray, aad: ByteArray): ByteArray
}

/**
 * AES-256-GCM; blob = iv(12) ‖ ciphertext+tag. The key comes from [key] (the
 * Keystore in the app). With [callerProvidedIv] false the provider draws the IV
 * (AndroidKeyStore keys with randomized encryption refuse a caller's IV).
 */
open class AesGcmValueCipher(
    private val key: () -> SecretKey,
    private val callerProvidedIv: Boolean = true,
) : ValueCipher {
    private val random = SecureRandom()

    override fun encrypt(plain: ByteArray, aad: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        if (callerProvidedIv) {
            cipher.init(Cipher.ENCRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, ByteArray(IV_LEN).also(random::nextBytes)))
        } else {
            cipher.init(Cipher.ENCRYPT_MODE, key())
        }
        check(cipher.iv.size == IV_LEN) { "unexpected IV length" }
        cipher.updateAAD(aad)
        val ct = cipher.doFinal(plain)
        return cipher.iv + ct
    }

    override fun decrypt(blob: ByteArray, aad: ByteArray): ByteArray {
        if (blob.size < IV_LEN + TAG_BITS / 8) throw GeneralSecurityException("blob too short")
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, blob, 0, IV_LEN))
        cipher.updateAAD(aad)
        return cipher.doFinal(blob, IV_LEN, blob.size - IV_LEN)
    }

    companion object {
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_LEN = 12
        private const val TAG_BITS = 128
    }
}

/**
 * The app's storage cipher: an AndroidKeyStore AES-256-GCM key `kaiku_storage_v1`
 * without user authentication (tokens must refresh on the lock screen),
 * StrongBox-backed where available. The key never leaves the keystore.
 */
class KeystoreValueCipher(context: Context) :
    AesGcmValueCipher({ storageKey(context.applicationContext) }, callerProvidedIv = false) {
    private companion object {
        const val ALIAS = "kaiku_storage_v1"
        val lock = Any()

        fun storageKey(context: Context): SecretKey = synchronized(lock) {
            val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
            (ks.getKey(ALIAS, null) as? SecretKey) ?: create(context)
        }

        fun create(context: Context): SecretKey {
            val strongBox = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
                context.packageManager.hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)
            return try {
                generate(strongBox)
            } catch (e: Exception) {
                val unavailable = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
                    generateSequence<Throwable>(e) { it.cause }.any { it is StrongBoxUnavailableException }
                if (strongBox && unavailable) generate(false) else throw e
            }
        }

        fun generate(strongBox: Boolean): SecretKey {
            val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .apply { if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) setIsStrongBoxBacked(true) }
                .build()
            return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
                init(spec)
                generateKey()
            }
        }
    }
}
