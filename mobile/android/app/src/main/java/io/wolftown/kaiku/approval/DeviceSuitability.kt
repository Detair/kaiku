package io.wolftown.kaiku.approval

import android.app.KeyguardManager
import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import androidx.biometric.BiometricManager
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.spec.ECGenParameterSpec
import java.util.logging.Logger

/** Whether this phone can be a key, checked before anyone scans or types anything (B.1). */
interface DeviceSuitabilityChecker {
    fun check(): UvDecision
}

/**
 * Screen lock first, then a probe key `kaiku_probe` (no auth, deleted at once)
 * whose [KeyInfo] must report a TEE or StrongBox (API 31+) or secure hardware
 * (below); then [UvPolicy].
 */
class AndroidDeviceSuitabilityChecker(private val context: Context) : DeviceSuitabilityChecker {

    private companion object {
        const val PROBE = "kaiku_probe"
        val logger: Logger = Logger.getLogger("DeviceSuitability")

        /** One probe at a time: two checks sharing the alias would delete each other's probe key. */
        val probeLock = Any()
    }

    override fun check(): UvDecision {
        val keyguard = context.getSystemService(KeyguardManager::class.java)
        if (keyguard == null || !keyguard.isDeviceSecure) return UvDecision.Unsuitable(UnsuitableReason.NO_SCREEN_LOCK)
        if (!hardwareBacked()) return UvDecision.Unsuitable(UnsuitableReason.NO_HARDWARE_KEYSTORE)
        val strongBiometric = BiometricManager.from(context)
            .canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS
        return UvPolicy.choose(Build.VERSION.SDK_INT, deviceSecure = true, strongBiometric = strongBiometric)
    }

    private fun hardwareBacked(): Boolean = synchronized(probeLock) {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        try {
            val pair = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore").run {
                initialize(
                    KeyGenParameterSpec.Builder(PROBE, KeyProperties.PURPOSE_SIGN)
                        .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                        .setDigests(KeyProperties.DIGEST_SHA256)
                        .build(),
                )
                generateKeyPair()
            }
            val info = KeyFactory.getInstance(pair.private.algorithm, "AndroidKeyStore")
                .getKeySpec(pair.private, KeyInfo::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                info.securityLevel == KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT ||
                    info.securityLevel == KeyProperties.SECURITY_LEVEL_STRONGBOX
            } else {
                @Suppress("DEPRECATION")
                info.isInsideSecureHardware
            }
        } catch (e: Exception) {
            logger.warning("Keystore probe failed: ${e.javaClass.simpleName}")
            false
        } finally {
            runCatching { if (ks.containsAlias(PROBE)) ks.deleteEntry(PROBE) }
        }
    }
}
