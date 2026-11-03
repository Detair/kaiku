package io.wolftown.kaiku.ui.auth

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.settings.findActivity

/**
 * "Sign in with passkey" (F1): first on the login screen, 48 dp. [busy]: a
 * passkey sign-in runs (its label); [enabled]: false while any other sign-in
 * (password, QR, SSO) runs, so the passkey cannot start beside it.
 */
@Composable
fun PasskeySignInButton(
    viewModel: LoginViewModel,
    busy: Boolean,
    enabled: Boolean,
    modifier: Modifier = Modifier,
) {
    val activity = LocalContext.current.findActivity()
    Button(
        onClick = { activity?.let(viewModel::onPasskeySignIn) },
        enabled = enabled && !busy && activity != null,
        modifier = modifier.fillMaxWidth().heightIn(min = 48.dp),
    ) {
        Text(stringResource(if (busy) R.string.passkeys_login_busy else R.string.passkeys_login_button))
    }
}
