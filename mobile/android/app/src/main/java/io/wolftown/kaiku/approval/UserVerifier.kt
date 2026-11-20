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
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import io.wolftown.kaiku.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import java.security.Signature
import java.util.UUID
import java.util.logging.Logger
import kotlin.coroutines.resume

/**
 * The UV prompt: title "Confirm it's you" (or a screen's own), subtitle = what is being confirmed,
 * [description] = an optional sentence more (for example why a second prompt follows).
 */
data class UvPrompt(@StringRes val title: Int, val subtitle: String, val description: String? = null)

sealed interface UvResult {
    class Signed(val der: ByteArray) : UvResult
    data object Cancelled : UvResult
    data object LockedOut : UvResult
    data object KeyInvalidated : UvResult
    data class Failed(val code: String) : UvResult
}

/**
 * Asks the phone's own screen lock (never a Kaiku PIN) and signs [message] whole with K_approve.
 * [keyId] names the key the caller holds: with another key in the alias nothing is signed
 * (`Failed("KEY_CHANGED")`).
 */
interface UserVerifier {
    suspend fun sign(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray, keyId: String): UvResult
}

/** The result when the alias no longer holds the key the caller asked to sign with. */
internal val KEY_CHANGED = UvResult.Failed("KEY_CHANGED")

/**
 * Before any prompt (B.1): no key, or a phone without a secure screen lock, is an
 * invalidated key. Android invalidates a user-authenticated key for good when
 * the secure lock screen is turned off, so this never needs the key itself
 * (whose lookup may then fail in other ways) and never prompts. Null: go on.
 */
internal fun uvPreflight(hasKey: Boolean, deviceSecure: Boolean): UvResult? =
    if (!hasKey || !deviceSecure) UvResult.KeyInvalidated else null

/** Initialises, signs and maps key failures; used right after the user was verified (or for a per-use key's CryptoObject). */
internal fun signNow(keyStore: ApprovalKeyStore, message: ByteArray, keyId: String): UvResult = try {
    finish(keyStore.signature(keyId), message)
} catch (_: KeyInvalidatedException) {
    UvResult.KeyInvalidated
} catch (_: NoApprovalKeyException) {
    UvResult.KeyInvalidated
} catch (_: KeyChangedException) {
    KEY_CHANGED
} catch (e: UserNotAuthenticatedException) {
    throw e // the window closed: the caller asks once more
} catch (e: Exception) {
    if (isInvalidatedKeyFailure(e)) UvResult.KeyInvalidated else UvResult.Failed("SIGN_FAILED")
}

/**
 * The signature a per-use key's prompt unlocked ([s]; null when the prompt gave none back), applied
 * to [message]. A key that Android invalidated meanwhile says so, as in [signNow].
 */
internal fun finishPerUse(s: Signature?, message: ByteArray): UvResult {
    if (s == null) return UvResult.Failed("UV_NO_CRYPTO_OBJECT")
    return try {
        finish(s, message)
    } catch (e: Exception) {
        if (isInvalidatedKeyFailure(e)) UvResult.KeyInvalidated else UvResult.Failed("SIGN_FAILED")
    }
}

/**
 * Shows a prompt and waits for its result, but never for a prompt that cannot be shown or whose
 * screen is gone. [blocked] is asked right before [show]: an activity that is finishing, destroyed
 * or has saved its state cannot show one (Android drops it without a word), so the answer is
 * [UvResult.Cancelled] at once. [show] starts the prompt and hands its result to the callback
 * once; [withdraw] takes the prompt back. When [lifecycle] (the activity the prompt was started
 * for) is destroyed first, the prompt is withdrawn and the answer is [UvResult.Cancelled]. On the
 * main thread.
 */
internal suspend fun awaitPrompt(
    lifecycle: Lifecycle,
    blocked: () -> Boolean,
    show: (done: (UvResult) -> Unit) -> Unit,
    withdraw: () -> Unit,
): UvResult {
    if (blocked() || lifecycle.currentState == Lifecycle.State.DESTROYED) return UvResult.Cancelled
    var observer: LifecycleEventObserver? = null
    try {
        return suspendCancellableCoroutine { cont ->
            observer = LifecycleEventObserver { _, event ->
                if (event == Lifecycle.Event.ON_DESTROY && cont.isActive) {
                    quietly(withdraw)
                    cont.resume(UvResult.Cancelled)
                }
            }.also(lifecycle::addObserver)
            cont.invokeOnCancellation { quietly(withdraw) }
            show { r -> if (cont.isActive) cont.resume(r) }
        }
    } finally {
        observer?.let(lifecycle::removeObserver)
    }
}

/** Taking a prompt back is best effort: its screen may already be gone. */
private inline fun quietly(block: () -> Unit) {
    try {
        block()
    } catch (_: RuntimeException) {
    }
}

/** An activity that cannot show a prompt now: finishing, destroyed, or its state already saved. */
internal fun FragmentActivity.cannotPrompt(): Boolean = isFinishing || isDestroyed || supportFragmentManager.isStateSaved

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

    override suspend fun sign(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray, keyId: String): UvResult =
        withContext(Dispatchers.Main) {
            val deviceSecure = activity.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true
            uvPreflight(keyStore.hasKey(), deviceSecure)?.let { return@withContext it }
            val uvClass = try {
                keyStore.keyUvClass(keyId)
            } catch (_: KeyChangedException) {
                return@withContext KEY_CHANGED
            } catch (_: KeyInvalidatedException) {
                return@withContext UvResult.KeyInvalidated
            } catch (_: NoApprovalKeyException) {
                return@withContext UvResult.KeyInvalidated
            }
            when (uvClass) {
                UvClass.WINDOW_10S -> signInWindow(activity, prompt, message, keyId)
                else -> signPerUse(activity, prompt, message, keyId)
            }
        }

    private suspend fun signPerUse(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray, keyId: String): UvResult {
        // The handle the prompt unlocks is the handle of the key the caller named.
        val signature = try {
            keyStore.signature(keyId)
        } catch (_: KeyInvalidatedException) {
            return UvResult.KeyInvalidated
        } catch (_: NoApprovalKeyException) {
            return UvResult.KeyInvalidated
        } catch (_: KeyChangedException) {
            return KEY_CHANGED
        }
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(activity.getString(prompt.title))
            .setSubtitle(prompt.subtitle)
            .apply { prompt.description?.let(::setDescription) }
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
        var biometricPrompt: BiometricPrompt? = null
        return awaitPrompt(
            lifecycle = activity.lifecycle,
            blocked = activity::cannotPrompt,
            show = { done ->
                biometricPrompt = BiometricPrompt(
                    activity,
                    ContextCompat.getMainExecutor(activity),
                    object : BiometricPrompt.AuthenticationCallback() {
                        override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                            val out = finishPerUse(result.cryptoObject?.signature, message)
                            if (out is UvResult.Failed) logger.warning("Signing failed: ${out.code}")
                            done(out)
                        }

                        override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                            done(mapBiometricError(errorCode))
                        }
                    },
                ).also { it.authenticate(info, BiometricPrompt.CryptoObject(signature)) }
            },
            withdraw = { biometricPrompt?.cancelAuthentication() },
        )
    }

    private suspend fun signInWindow(activity: FragmentActivity, prompt: UvPrompt, message: ByteArray, keyId: String): UvResult {
        repeat(2) {
            when (val confirmed = confirmCredential(activity, prompt)) {
                null -> Unit
                else -> return confirmed
            }
            try {
                return signNow(keyStore, message, keyId)
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
        val text = listOfNotNull(prompt.subtitle, prompt.description).joinToString("\n")
        val intent = keyguard?.createConfirmDeviceCredentialIntent(activity.getString(prompt.title), text)
            ?: return UvResult.KeyInvalidated
        var launcher: androidx.activity.result.ActivityResultLauncher<android.content.Intent>? = null
        // "Confirmed" travels as a Signed placeholder through the wait; every other result is returned.
        val confirmed = UvResult.Signed(ByteArray(0))
        val r = awaitPrompt(
            lifecycle = activity.lifecycle,
            blocked = activity::cannotPrompt,
            show = { done ->
                val key = "kaiku_uv_" + UUID.randomUUID()
                launcher = activity.activityResultRegistry.register(key, ActivityResultContracts.StartActivityForResult()) { res ->
                    launcher?.unregister()
                    done(if (res.resultCode == Activity.RESULT_OK) confirmed else UvResult.Cancelled)
                }
                launcher?.launch(intent)
            },
            withdraw = { launcher?.unregister() },
        )
        return if (r === confirmed) null else r
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
