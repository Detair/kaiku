package io.wolftown.kaiku.ui.auth

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.QrImage
import io.wolftown.kaiku.ui.shared.SecureScreen

/**
 * Signing in on a phone that is not (yet) a key (Task B14): the QR for the key
 * phone (F2 "Sign in with your other phone", or F3's confirmation of a password
 * or SSO sign-in), the TOTP step (F4) and how an attempt ended. Back cancels
 * the attempt. There is no typed sign-in code (F2).
 */
@Composable
fun PhoneSignInScreen(
    screen: LoginAttemptUi,
    busy: Boolean,
    onPhoneNotAtHand: () -> Unit,
    onSubmitTotp: (String) -> Unit,
    onTryAgain: () -> Unit,
    onLeave: () -> Unit,
) {
    BackHandler(onBack = onLeave)
    // The QR carries a sign-in token and the TOTP step a code: no screenshots or
    // screen recording on any step (one reference-counted protection for all).
    SecureScreen()
    when (screen) {
        is LoginAttemptUi.ShowQr -> ShowQr(screen, onPhoneNotAtHand, onLeave)
        is LoginAttemptUi.Totp -> TotpStep(screen, busy, onSubmitTotp, onLeave)
        is LoginAttemptUi.Ended -> KeyScreen {
            KeyNotice(stringResource(screen.msg), warning = true)
            KeyPrimaryButton(stringResource(R.string.auth_phone_try_again), onClick = onTryAgain)
            KeySecondaryButton(stringResource(R.string.auth_back_to_sign_in), onClick = onLeave)
        }
    }
}

@Composable
private fun ShowQr(qr: LoginAttemptUi.ShowQr, onPhoneNotAtHand: () -> Unit, onLeave: () -> Unit) {
    KeyScreen {
        val other = qr.via == AttemptVia.OTHER_PHONE
        KeyTitle(stringResource(if (other) R.string.auth_phone_title_other else R.string.auth_phone_title_confirm))
        KeyBody(stringResource(if (other) R.string.auth_phone_scan_other else R.string.auth_phone_scan_confirm))
        if (qr.payload.isEmpty()) {
            CircularProgressIndicator()
        } else {
            QrImage(
                payload = qr.payload,
                contentDescription = stringResource(R.string.auth_phone_qr_description),
                modifier = Modifier.fillMaxWidth().widthIn(max = 360.dp),
            )
        }
        KeyBody(pluralStringResource(R.plurals.auth_phone_minutes_left, qr.minutesLeft, qr.minutesLeft), secondary = true)
        if (qr.totpAvailable) {
            KeySecondaryButton(stringResource(R.string.auth_phone_not_at_hand), onClick = onPhoneNotAtHand)
        }
        KeySecondaryButton(stringResource(R.string.common_cancel), onClick = onLeave)
    }
}

@Composable
private fun TotpStep(step: LoginAttemptUi.Totp, busy: Boolean, onSubmit: (String) -> Unit, onLeave: () -> Unit) {
    // Memory only: an authenticator code never goes into saved instance state.
    var code by remember { mutableStateOf("") }
    KeyScreen {
        KeyTitle(stringResource(R.string.auth_totp_title))
        KeyBody(stringResource(R.string.auth_totp_body))
        OutlinedTextField(
            value = code,
            onValueChange = { v -> code = v.filter { it.isDigit() || it == ' ' }.take(8) },
            label = { Text(stringResource(R.string.auth_totp_label)) },
            singleLine = true,
            textStyle = TextStyle(fontSize = 32.sp, letterSpacing = 4.sp),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.NumberPassword, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { if (!busy && code.any(Char::isDigit)) onSubmit(code) }),
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        )
        KeyBody(stringResource(R.string.auth_totp_hint), secondary = true)
        step.error?.let {
            Text(stringResource(it), color = MaterialTheme.colorScheme.error, fontSize = 18.sp)
        }
        KeyPrimaryButton(
            stringResource(R.string.auth_sign_in),
            onClick = { onSubmit(code) },
            enabled = code.any(Char::isDigit),
            working = busy,
        )
        KeySecondaryButton(stringResource(R.string.common_cancel), onClick = onLeave, enabled = !busy)
    }
}
