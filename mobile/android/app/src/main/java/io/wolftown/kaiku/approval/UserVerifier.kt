package io.wolftown.kaiku.approval

import android.app.Activity
import android.app.KeyguardManager
import android.os.Build
import android.security.keystore.UserNotAuthenticatedException
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import java.security.Signature
import java.util.UUID
import java.util.logging.Logger
import kotlin.coroutines.resume

/** The UV prompt: title "Confirm it's you" (or a screen's own), subtitle = what is being confirmed. */
data class UvPrompt(@StringRes val title: Int, val subtitle: String)

sealed interface UvResult {
    class Signed(val der: ByteArray) : UvResult
    data object Cancelled : UvResult
    data object LockedOut : UvResult
    data object KeyInvalidated : UvResult
    data class Failed(val code: String) : UvResult
}

/** Asks the phone's own screen lock (never a Kaiku PIN) and signs [message] whole with K_approve. */
interface UserVerifier {
    suspend fun sign(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray): UvResult
}

/**
 * Before any prompt (B.1): no key, or a phone without a secure screen lock, is an
 * invalidated key. Android invalidates a user-authenticated key for good when
 * the secure lock screen is turned off, so this never needs the key itself
 * (whose lookup may then fail in other ways) and never prompts. Null: go on.
 */
internal fun uvPreflight(hasKey: Boolean, deviceSecure: Boolean): UvResult? =
    if (!hasKey || !deviceSecure) UvResult.KeyInvalidated else null

/** Initialises, signs and maps key failures; used right after the user was verified (or for a per-use key's CryptoObject). */
internal fun signNow(keyStore: ApprovalKeyStore, message: ByteArray): UvResult = try {
    finish(keyStore.signature(), message)
} catch (_: KeyInvalidatedException) {
    UvResult.KeyInvalidated
} catch (_: NoApprovalKeyException) {
    UvResult.KeyInvalidated
}

/** Signs [message] with an initialised (and, for per-use keys, authenticated) signature; never prehashed. */
internal fun finish(signature: Signature, message: ByteArray): UvResult.Signed {
    signature.update(message)
    return UvResult.Signed(signature.sign())
}

/**
 * Per-use keys: a [BiometricPrompt] with a CryptoObject for every signature
 * (API 30+: strong biometric or screen lock; API 26–29: strong biometric with a
 * "Cancel" button). Window keys (API 26–29 PIN only): the screen-lock prompt
 * from `createConfirmDeviceCredentialIntent`, then the signature at once.
 */
class BiometricUserVerifier(private val keyStore: ApprovalKeyStore) : UserVerifier {

    private companion object {
        val logger: Logger = Logger.getLogger("UserVerifier")
    }

    override suspend fun sign(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray): UvResult =
        withContext(Dispatchers.Main) {
            val deviceSecure = activity.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true
            uvPreflight(keyStore.hasKey(), deviceSecure)?.let { return@withContext it }
            val uvClass = try {
                keyStore.keyUvClass()
            } catch (_: KeyInvalidatedException) {
                return@withContext UvResult.KeyInvalidated
            }
            when (uvClass) {
                UvClass.WINDOW_10S -> signInWindow(activity, prompt, message)
                else -> signPerUse(activity, prompt, message)
            }
        }

    private suspend fun signPerUse(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray): UvResult {
        val signature = try {
            keyStore.signature()
        } catch (_: KeyInvalidatedException) {
            return UvResult.KeyInvalidated
        } catch (_: NoApprovalKeyException) {
            return UvResult.KeyInvalidated
        }
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(activity.getString(prompt.title))
            .setSubtitle(prompt.subtitle)
            .setConfirmationRequired(true)
            .apply {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    setAllowedAuthenticators(
                        BiometricPromptAuthenticators.BIOMETRIC_STRONG or BiometricPromptAuthenticators.DEVICE_CREDENTIAL,
                    )
                } else {
                    setAllowedAuthenticators(BiometricPromptAuthenticators.BIOMETRIC_STRONG)
                    setNegativeButtonText(activity.getString(R.string.common_cancel))
                }
            }
            .build()
        return suspendCancellableCoroutine { cont ->
            val biometricPrompt = BiometricPrompt(
                activity,
                ContextCompat.getMainExecutor(activity),
                object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        val s = result.cryptoObject?.signature
                        val out = if (s == null) {
                            UvResult.Failed("UV_NO_CRYPTO_OBJECT")
                        } else {
                            try {
                                finish(s, message)
                            } catch (e: Exception) {
                                logger.warning("Signing failed: ${e.javaClass.simpleName}")
                                UvResult.Failed("SIGN_FAILED")
                            }
                        }
                        if (cont.isActive) cont.resume(out)
                    }

                    override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                        if (cont.isActive) cont.resume(mapBiometricError(errorCode))
                    }
                },
            )
            cont.invokeOnCancellation { biometricPrompt.cancelAuthentication() }
            biometricPrompt.authenticate(info, BiometricPrompt.CryptoObject(signature))
        }
    }

    private suspend fun signInWindow(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray): UvResult {
        repeat(2) {
            when (val confirmed = confirmCredential(activity, prompt)) {
                null -> Unit
                else -> return confirmed
            }
            try {
                return signNow(keyStore, message)
            } catch (_: UserNotAuthenticatedException) {
                // The window closed before the signature; ask once more.
            }
        }
        return UvResult.Failed("UV_WINDOW_EXPIRED")
    }

    /** Null when the screen lock was confirmed, else the result to return. */
    private suspend fun confirmCredential(activity: FragmentActivity, prompt: UvPrompt): UvResult? {
        val keyguard = activity.getSystemService(KeyguardManager::class.java)
        @Suppress("DEPRECATION")
        // No intent means no secure screen lock any more: Android has invalidated the key (B.1).
        val intent = keyguard?.createConfirmDeviceCredentialIntent(activity.getString(prompt.title), prompt.subtitle)
            ?: return UvResult.KeyInvalidated
        return suspendCancellableCoroutine { cont ->
            val key = "kaiku_uv_" + UUID.randomUUID()
            var launcher: androidx.activity.result.ActivityResultLauncher<android.content.Intent>? = null
            launcher = activity.activityResultRegistry.register(key, ActivityResultContracts.StartActivityForResult()) { r ->
                launcher?.unregister()
                if (cont.isActive) cont.resume(if (r.resultCode == Activity.RESULT_OK) null else UvResult.Cancelled)
            }
            cont.invokeOnCancellation { launcher.unregister() }
            launcher.launch(intent)
        }
    }
}

/** `androidx.biometric.BiometricManager.Authenticators`, as the prompt takes them. */
private object BiometricPromptAuthenticators {
    const val BIOMETRIC_STRONG = androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG
    const val DEVICE_CREDENTIAL = androidx.biometric.BiometricManager.Authenticators.DEVICE_CREDENTIAL
}

/**
 * BiometricPrompt error codes → results: lockouts, cancels (user, negative button, system),
 * no screen lock any more (the key is invalidated, B.1), others failed. `ERROR_NO_BIOMETRICS`
 * stays a failure: with `setInvalidatedByBiometricEnrollment(false)` the key survives the
 * removal of every fingerprint and works again once one is added.
 */
internal fun mapBiometricError(errorCode: Int): UvResult = when (errorCode) {
    BiometricPrompt.ERROR_LOCKOUT, BiometricPrompt.ERROR_LOCKOUT_PERMANENT -> UvResult.LockedOut
    BiometricPrompt.ERROR_USER_CANCELED, BiometricPrompt.ERROR_NEGATIVE_BUTTON, BiometricPrompt.ERROR_CANCELED -> UvResult.Cancelled
    BiometricPrompt.ERROR_NO_DEVICE_CREDENTIAL -> UvResult.KeyInvalidated
    else -> UvResult.Failed("UV_ERROR_$errorCode")
}
