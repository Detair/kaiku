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
import java.security.MessageDigest
import java.util.Base64

/** The phone's key K_approve: the key exists but Android invalidated it (screen lock removed). */
class KeyInvalidatedException : Exception("approval key invalidated")

/** No approval key on this phone. */
class NoApprovalKeyException : Exception("no approval key")

/** The alias holds another key than the one the caller asked for: nothing is signed or deleted. */
class KeyChangedException : Exception("approval key changed")

/**
 * Whether a Keystore failure is Android's permanent invalidation of the key: a
 * [KeyPermanentlyInvalidatedException] anywhere in the cause chain. Only this
 * forgets the key (Fable R2). `android.security.KeyStoreException` has no public
 * error code for it (API 33–35 list none), and a message that merely reads like
 * one ("…invalidated…", "…auth credentials…") is not proof: such failures stay
 * ordinary, non-destructive Keystore failures. A removed screen lock is caught
 * before any key use by [uvPreflight] (no secure lock screen).
 */
internal fun isInvalidatedKeyFailure(e: Throwable): Boolean =
    generateSequence(e) { it.cause }.take(16).any { it is KeyPermanentlyInvalidatedException }

/** Names a key by its public key: base64url of the SHA-256 of its SPKI. */
object KeyId {
    fun of(spki: ByteArray): String =
        Base64.getUrlEncoder().withoutPadding().encodeToString(MessageDigest.getInstance("SHA-256").digest(spki))
}

/** A new key: its SPKI DER, the attestation chain (leaf first) and how it verifies its user. */
class GeneratedKey(val spki: ByteArray, val attestationChain: List<ByteArray>, val uvClass: UvClass) {
    val keyId: String get() = KeyId.of(spki)
}

/**
 * Runs [make], which creates the key under the alias and reads it back. If it cannot return the
 * key (the read-back failed after the alias was created), the alias is deleted again, so a failed
 * generation never leaves a key nobody holds a handle for.
 */
internal inline fun newKeyOrNone(deleteAlias: () -> Unit, make: () -> GeneratedKey): GeneratedKey = try {
    make()
} catch (e: Throwable) {
    try {
        deleteAlias()
    } catch (_: Exception) {
        // The original failure is the one to report.
    }
    throw e
}

/**
 * The one approval key K_approve (P-256, non-exportable, hardware-backed, UV on
 * every use), under one alias. Every operation that uses or removes the key names the key it
 * expects ([KeyId]); with another key in the alias nothing
 * happens. [generate] refuses to replace an existing key, and a new alias it cannot hand back
 * (its certificate cannot be read) is deleted again before it fails. Implementations run every
 * operation under one monitor, so a check and its use are never split by another caller.
 */
interface ApprovalKeyStore {
    fun hasKey(): Boolean
    fun generate(attestationChallenge: ByteArray, d: UvDecision.Use): GeneratedKey
    fun publicKeySpki(): ByteArray?

    /** How the existing key verifies its user, read from the key itself; null without a key. */
    fun keyUvClass(): UvClass?

    /**
     * Deletes the key only if it is [expectedKeyId] (null: an alias whose public key cannot be
     * read). True when that key is gone afterwards; false, and nothing deleted, with another key.
     */
    fun delete(expectedKeyId: String?): Boolean

    /**
     * A `SHA256withECDSA` [Signature] initialised for signing with the key [expectedKeyId].
     * Throws [KeyChangedException] when the alias holds another key, [KeyInvalidatedException],
     * [NoApprovalKeyException], or for a window key outside its window
     * `android.security.keystore.UserNotAuthenticatedException`.
     */
    fun signature(expectedKeyId: String): Signature
}

/** The [KeyId] of the key in the alias; null without a readable one. */
fun ApprovalKeyStore.keyId(): String? = publicKeySpki()?.let(KeyId::of)

class AndroidApprovalKeyStore(private val context: Context) : ApprovalKeyStore {

    companion object {
        const val ALIAS = "kaiku_approve_v1"
        private const val PROVIDER = "AndroidKeyStore"
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

    @Synchronized
    override fun hasKey(): Boolean = keyStore().containsAlias(ALIAS)

    @Synchronized
    override fun generate(attestationChallenge: ByteArray, d: UvDecision.Use): GeneratedKey {
        check(!hasKey()) { "approval key exists; delete it explicitly first" }
        // No key was there, so whatever the alias holds from here on is this call's own.
        return newKeyOrNone(deleteAlias = { keyStore().run { if (containsAlias(ALIAS)) deleteEntry(ALIAS) } }) {
            val strongBox = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
                context.packageManager.hasSystemFeature(PackageManager.FEATURE_STRONGBOX_KEYSTORE)
            try {
                generatePair(attestationChallenge, d, strongBox)
            } catch (e: Exception) {
                if (!strongBox || !isStrongBoxUnavailable(e)) throw e
                generatePair(attestationChallenge, d, strongBox = false)
            }
            val ks = keyStore()
            val chain = ks.getCertificateChain(ALIAS)?.map { it.encoded } ?: emptyList()
            val spki = ks.getCertificate(ALIAS)?.publicKey?.encoded
                ?: throw java.security.KeyStoreException("no certificate for the new key")
            GeneratedKey(spki, chain, d.uvClass)
        }
    }

    private fun isStrongBoxUnavailable(e: Exception): Boolean =
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
            generateSequence<Throwable>(e) { it.cause }.any { it is StrongBoxUnavailableException }

    private fun generatePair(challenge: ByteArray, d: UvDecision.Use, strongBox: Boolean) {
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

    @Synchronized
    override fun publicKeySpki(): ByteArray? = keyStore().getCertificate(ALIAS)?.publicKey?.encoded

    @Synchronized
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

    @Synchronized
    override fun delete(expectedKeyId: String?): Boolean {
        val ks = keyStore()
        if (!ks.containsAlias(ALIAS)) return true
        if (ks.getCertificate(ALIAS)?.publicKey?.encoded?.let(KeyId::of) != expectedKeyId) return false
        ks.deleteEntry(ALIAS)
        return true
    }

    @Synchronized
    override fun signature(expectedKeyId: String): Signature {
        val key = privateKey() ?: throw NoApprovalKeyException()
        // Checked under the same monitor that guards generating and deleting: the handle
        // initialised below is the handle of exactly this key.
        if (publicKeySpki()?.let(KeyId::of) != expectedKeyId) throw KeyChangedException()
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
