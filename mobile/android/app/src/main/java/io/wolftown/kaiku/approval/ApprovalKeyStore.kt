package io.wolftown.kaiku.approval

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.GeneralSecurityException
import java.security.PrivateKey
import java.security.Signature
import java.security.spec.ECGenParameterSpec

/** The phone's key K_approve: the key exists but Android invalidated it (screen lock removed). */
class KeyInvalidatedException : Exception("approval key invalidated")

/** No approval key on this phone. */
class NoApprovalKeyException : Exception("no approval key")

/**
 * Whether a Keystore lookup failure means Android invalidated the key: keystore2
 * reports `KEY_PERMANENTLY_INVALIDATED` at `getKey()` as an
 * [java.security.UnrecoverableKeyException] carrying only the message ("User
 * changed or deleted their auth credentials"), older Keystores as
 * [KeyPermanentlyInvalidatedException]. Any other failure (a busy or broken
 * Keystore) is not an invalidation: the key is never forgotten for it.
 */
internal fun isInvalidatedKeyFailure(e: Throwable): Boolean = generateSequence(e) { it.cause }.any { t ->
    t is KeyPermanentlyInvalidatedException ||
        t.message?.let { m -> m.contains("invalidated", ignoreCase = true) || m.contains("auth credentials", ignoreCase = true) } == true
}

/** A new key: its SPKI DER, the attestation chain (leaf first) and how it verifies its user. */
class GeneratedKey(val spki: ByteArray, val attestationChain: List<ByteArray>, val uvClass: UvClass)

/**
 * The one approval key K_approve (P-256, non-exportable, hardware-backed, UV on
 * every use). [generate] refuses to replace an existing key: replacing is
 * always an explicit [delete] by the caller, never silent.
 */
interface ApprovalKeyStore {
    fun hasKey(): Boolean
    fun generate(attestationChallenge: ByteArray, d: UvDecision.Use): GeneratedKey
    fun publicKeySpki(): ByteArray?

    /** How the existing key verifies its user, read from the key itself; null without a key. */
    fun keyUvClass(): UvClass?
    fun delete()

    /**
     * A `SHA256withECDSA` [Signature] initialised for signing. Throws
     * [KeyInvalidatedException], [NoApprovalKeyException], or for a window key
     * outside its window `android.security.keystore.UserNotAuthenticatedException`.
     */
    fun signature(): Signature
}

class AndroidApprovalKeyStore(private val context: Context) : ApprovalKeyStore {

    companion object {
        const val ALIAS = "kaiku_approve_v1"
        private const val PROVIDER = "AndroidKeyStore"
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

    override fun hasKey(): Boolean = keyStore().containsAlias(ALIAS)

    override fun generate(attestationChallenge: ByteArray, d: UvDecision.Use): GeneratedKey {
        check(!hasKey()) { "approval key exists; delete it explicitly first" }
        val strongBox = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
            context.packageManager.hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)
        try {
            generateKey(attestationChallenge, d, strongBox)
        } catch (e: Exception) {
            if (!strongBox || !isStrongBoxUnavailable(e)) throw e
            generateKey(attestationChallenge, d, strongBox = false)
        }
        val ks = keyStore()
        val chain = ks.getCertificateChain(ALIAS)?.map { it.encoded } ?: emptyList()
        val spki = ks.getCertificate(ALIAS).publicKey.encoded
        return GeneratedKey(spki, chain, d.uvClass)
    }

    private fun isStrongBoxUnavailable(e: Exception): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
            generateSequence<Throwable>(e) { it.cause }.any { it is StrongBoxUnavailableException }

    private fun generateKey(challenge: ByteArray, d: UvDecision.Use, strongBox: Boolean) {
        val p = keySpecParams(d)
        val builder = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_SIGN)
            .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(p.invalidatedByBiometricEnrollment)
            .setAttestationChallenge(challenge)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && p.authTypes != null) {
            val types = (if (p.authTypes and KeyAuthTypes.BIOMETRIC_STRONG != 0) KeyProperties.AUTH_BIOMETRIC_STRONG else 0) or
                (if (p.authTypes and KeyAuthTypes.DEVICE_CREDENTIAL != 0) KeyProperties.AUTH_DEVICE_CREDENTIAL else 0)
            builder.setUserAuthenticationParameters(p.validitySeconds, types)
        } else {
            @Suppress("DEPRECATION")
            builder.setUserAuthenticationValidityDurationSeconds(p.validitySeconds)
        }
        if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) builder.setIsStrongBoxBacked(true)
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, PROVIDER).run {
            initialize(builder.build())
            generateKeyPair()
        }
    }

    override fun publicKeySpki(): ByteArray? = keyStore().getCertificate(ALIAS)?.publicKey?.encoded

    override fun keyUvClass(): UvClass? {
        val key = privateKey() ?: return null
        val info = invalidationMapped { KeyFactory.getInstance(key.algorithm, PROVIDER).getKeySpec(key, KeyInfo::class.java) }
        val timeout = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            info.userAuthenticationValidityDurationSeconds.takeIf { it >= 0 } ?: 0
        } else {
            @Suppress("DEPRECATION")
            info.userAuthenticationValidityDurationSeconds
        }
        return if (timeout > 0) UvClass.WINDOW_10S else UvClass.PER_USE
    }

    override fun delete() {
        val ks = keyStore()
        if (ks.containsAlias(ALIAS)) ks.deleteEntry(ALIAS)
    }

    override fun signature(): Signature {
        val key = privateKey() ?: throw NoApprovalKeyException()
        return invalidationMapped { Signature.getInstance("SHA256withECDSA").apply { initSign(key) } }
    }

    /** The key, or null without one; an invalidated key's lookup failure → [KeyInvalidatedException]. */
    private fun privateKey(): PrivateKey? = invalidationMapped { keyStore().getKey(ALIAS, null) as? PrivateKey }

    /** Turns the Keystore's ways of saying "invalidated" into [KeyInvalidatedException]; other failures stay. */
    private inline fun <T> invalidationMapped(block: () -> T): T = try {
        block()
    } catch (_: KeyPermanentlyInvalidatedException) {
        throw KeyInvalidatedException()
    } catch (e: GeneralSecurityException) {
        if (isInvalidatedKeyFailure(e)) throw KeyInvalidatedException()
        throw e
    }
}
