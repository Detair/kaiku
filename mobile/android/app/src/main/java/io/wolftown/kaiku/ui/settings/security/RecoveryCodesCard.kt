package io.wolftown.kaiku.ui.settings.security

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.sp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.SecureScreen

@Composable
fun RecoveryCodesCard(left: Int, readOnly: Boolean, onCreate: () -> Unit) {
    Body(pluralStringResource(R.plurals.security_recovery_codes_left, left, left))
    if (!readOnly) Action(stringResource(R.string.security_recovery_codes_create), onClick = onCreate)
}

/** New recovery codes, shown once inside [SecureScreen] until "I've written them down". */
@Composable
fun RecoveryCodesShownOnce(codes: List<String>, onDone: () -> Unit) {
    SecureScreen()
    KeyScreen {
        KeyTitle(stringResource(R.string.security_recovery_codes))
        KeyNotice(stringResource(R.string.security_recovery_codes_new), warning = true)
        codes.forEach { Text(it, fontSize = 24.sp, fontFamily = FontFamily.Monospace, color = MaterialTheme.colorScheme.onBackground) }
        KeyPrimaryButton(stringResource(R.string.security_recovery_codes_done), onClick = onDone)
    }
}
