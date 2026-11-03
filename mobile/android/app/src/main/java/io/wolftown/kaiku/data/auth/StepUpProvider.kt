package io.wolftown.kaiku.data.auth

import android.app.Activity
import io.wolftown.kaiku.domain.model.StepUpGrant

/**
 * LP's Android step-up seam (C29, F7): one grant for one action and target.
 * LP implements it with a passkey ([PasskeyStepUp]); L2 binds the key device's
 * `device_stepup` signer (the first passkey of an account comes from there, DC1).
 */
interface StepUpProvider {
    suspend fun stepUp(activity: Activity, action: String, target: String): Result<StepUpGrant>
}

/** Marks the key device's [StepUpProvider] binding (L2; optional until then). */
@javax.inject.Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class DeviceStepUp
