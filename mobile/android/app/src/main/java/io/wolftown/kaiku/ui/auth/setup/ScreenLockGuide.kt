package io.wolftown.kaiku.ui.auth.setup

import android.content.ActivityNotFoundException
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle

/**
 * B.1: without a screen lock setup stops with a guided step: one button to the
 * security settings (and on API 30+ the fingerprint enrolment), the hint to
 * choose the PIN together, and a re-check (also on resume, by the caller).
 */
@Composable
fun ScreenLockGuide(onCheck: () -> Unit, onCancel: () -> Unit) {
    val context = LocalContext.current
    fun open(intent: Intent) {
        try {
            context.startActivity(intent)
        } catch (_: ActivityNotFoundException) {
            context.startActivity(Intent(Settings.ACTION_SETTINGS))
        }
    }
    KeyTitle(stringResource(R.string.setup_screen_lock_title))
    KeyBody(stringResource(R.string.setup_screen_lock_body))
    KeyNotice(stringResource(R.string.setup_screen_lock_hint), warning = false)
    KeyPrimaryButton(stringResource(R.string.setup_screen_lock_open), onClick = { open(Intent(Settings.ACTION_SECURITY_SETTINGS)) })
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        KeySecondaryButton(stringResource(R.string.setup_fingerprint_open), onClick = {
            open(
                Intent(Settings.ACTION_BIOMETRIC_ENROLL).putExtra(
                    Settings.EXTRA_BIOMETRIC_AUTHENTICATORS_ALLOWED,
                    androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG or
                        androidx.biometric.BiometricManager.Authenticators.DEVICE_CREDENTIAL,
                ),
            )
        })
    }
    KeySecondaryButton(stringResource(R.string.setup_screen_lock_check), onClick = onCheck)
    KeySecondaryButton(stringResource(R.string.common_cancel), onClick = onCancel)
}
