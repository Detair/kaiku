package io.wolftown.kaiku.approval

/** C26 `uv_class` of a key device. */
enum class UvClass(val wire: String) { PER_USE("per_use"), WINDOW_10S("window_10s") }

enum class UnsuitableReason { NO_SCREEN_LOCK, NO_HARDWARE_KEYSTORE }

/** How this phone verifies its user for every signature, or why it cannot be a key (spec B.1, CC1). */
sealed interface UvDecision {
    /** [authenticators] are the BiometricPrompt authenticators ([UvAuthenticators]). */
    data class Use(val uvClass: UvClass, val authenticators: Int, val api30: Boolean) : UvDecision
    data class Unsuitable(val reason: UnsuitableReason) : UvDecision
}

/** `androidx.biometric.BiometricManager.Authenticators` values (kept here so policy code is plain JVM). */
object UvAuthenticators {
    const val BIOMETRIC_STRONG = 0x000F
    const val DEVICE_CREDENTIAL = 0x8000
}

/** `android.security.keystore.KeyProperties.AUTH_*` values. */
object KeyAuthTypes {
    const val DEVICE_CREDENTIAL = 1
    const val BIOMETRIC_STRONG = 2
}

/**
 * B.1 policy: API 30+ per-use authentication by a strong biometric or the
 * screen lock; API 26–29 a strong biometric per use, or with only a PIN,
 * pattern or password a 10 s window opened by a credential prompt right before
 * each signature; no screen lock, no key. There is no possession-only class.
 */
object UvPolicy {
    fun choose(apiLevel: Int, deviceSecure: Boolean, strongBiometric: Boolean): UvDecision = when {
        !deviceSecure -> UvDecision.Unsuitable(UnsuitableReason.NO_SCREEN_LOCK)
        apiLevel >= 30 -> UvDecision.Use(
            UvClass.PER_USE,
            UvAuthenticators.BIOMETRIC_STRONG or UvAuthenticators.DEVICE_CREDENTIAL,
            api30 = true,
        )
        strongBiometric -> UvDecision.Use(UvClass.PER_USE, UvAuthenticators.BIOMETRIC_STRONG, api30 = false)
        else -> UvDecision.Use(UvClass.WINDOW_10S, UvAuthenticators.DEVICE_CREDENTIAL, api30 = false)
    }
}

/**
 * Keystore parameters for [UvDecision.Use]. [validitySeconds]: 0 with [authTypes]
 * (`setUserAuthenticationParameters`, API 30+), -1 for a biometric per use
 * (API 26–29), 10 for the credential window. Never invalidated by a new
 * fingerprint: a grandchild adding one must not destroy the key.
 */
data class KeySpecParams(
    val perUseAuth: Boolean,
    val validitySeconds: Int,
    val authTypes: Int?,
    val invalidatedByBiometricEnrollment: Boolean,
)

fun keySpecParams(d: UvDecision.Use): KeySpecParams = when {
    d.api30 -> KeySpecParams(true, 0, KeyAuthTypes.BIOMETRIC_STRONG or KeyAuthTypes.DEVICE_CREDENTIAL, false)
    d.uvClass == UvClass.PER_USE -> KeySpecParams(true, -1, null, false)
    else -> KeySpecParams(false, WINDOW_SECONDS, null, false)
}

/** The credential window of API 26–29 PIN-only phones (B.1: at most 10 s). */
const val WINDOW_SECONDS = 10
