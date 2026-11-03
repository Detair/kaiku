package io.wolftown.kaiku.ui.auth

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import android.content.Context
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R

/**
 * The sign-in screen on a phone that is not (yet) a key, in the Task B14
 * order: passkey (LP), "Sign in with your other phone", username and password,
 * SSO, "Set up this phone with a setup code", then "I lost my phone / forgot
 * my password" and "Secure my account" on the server's web pages (Custom Tab,
 * coordinator call 2). A running attempt shows [PhoneSignInScreen] instead.
 */
@Composable
fun LoginScreen(
    onNavigateToRegister: () -> Unit,
    onLoginSuccess: () -> Unit,
    onPasswordChangeRequired: () -> Unit = {},
    onChangeServer: () -> Unit = {},
    onSetUpPhone: () -> Unit = {},
    viewModel: LoginViewModel = hiltViewModel(),
    attemptViewModel: LoginAttemptViewModel = hiltViewModel(),
) {
    val context = LocalContext.current
    val uiState by viewModel.uiState.collectAsState()
    val attempt by attemptViewModel.state.collectAsState()
    val busy = uiState.isLoading || attempt.busy
    // Each ViewModel refuses a start while the other's sign-in runs (not only the buttons).
    SideEffect {
        viewModel.setOtherSignInRunning(attempt.busy)
        attemptViewModel.setPasskeyRunning(uiState.isLoading)
    }

    LaunchedEffect(uiState.loginSuccess, attempt.signedIn) {
        if (uiState.loginSuccess || attempt.signedIn) {
            viewModel.clearPassword()
            onLoginSuccess()
        }
    }

    LaunchedEffect(attempt.passwordChangeRequired) {
        if (attempt.passwordChangeRequired) {
            viewModel.clearPassword()
            attemptViewModel.onPasswordChangeNavigated()
            onPasswordChangeRequired()
        }
    }

    attempt.screen?.let { screen ->
        PhoneSignInScreen(
            screen = screen,
            busy = attempt.busy,
            onPhoneNotAtHand = attemptViewModel::phoneNotAtHand,
            onSubmitTotp = attemptViewModel::submitTotp,
            onTryAgain = attemptViewModel::tryAgain,
            onLeave = attemptViewModel::leave,
        )
        return
    }

    LaunchedEffect(uiState.changeServerRequested) {
        if (uiState.changeServerRequested) {
            viewModel.onChangeServerNavigated()
            onChangeServer()
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 48.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            text = stringResource(R.string.auth_login_title),
            style = MaterialTheme.typography.headlineMedium,
            color = MaterialTheme.colorScheme.primary
        )

        Spacer(modifier = Modifier.height(8.dp))

        Text(
            text = stringResource(R.string.auth_login_subtitle),
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )

        if (uiState.serverHost.isNotEmpty()) {
            Spacer(modifier = Modifier.height(4.dp))

            Text(
                text = stringResource(R.string.auth_login_server, uiState.serverHost),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }

        Spacer(modifier = Modifier.height(32.dp))

        // Passkeys first (F1).
        // Off while a password, QR or SSO sign-in runs, and those are off while it runs (`busy`).
        PasskeySignInButton(viewModel = viewModel, busy = uiState.passkeyLoading, enabled = !uiState.isLoading && !attempt.busy)
        uiState.passkeyErrorRes?.let { res ->
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                text = if (res == R.string.passkeys_error_ready_at) {
                    stringResource(res, io.wolftown.kaiku.ui.settings.formatReadyAt(uiState.passkeyReadyAt?.toString()))
                } else {
                    stringResource(res)
                },
                color = MaterialTheme.colorScheme.error,
            )
        }
        Spacer(modifier = Modifier.height(12.dp))

        // F2 on a second phone: the key phone scans this phone's code.
        OutlinedButton(
            onClick = attemptViewModel::startQr,
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
            enabled = !busy
        ) {
            Text(stringResource(R.string.auth_login_other_phone), color = MaterialTheme.colorScheme.onBackground)
        }

        Spacer(modifier = Modifier.height(24.dp))

        OutlinedTextField(
            value = uiState.username,
            onValueChange = viewModel::onUsernameChanged,
            label = { Text(stringResource(R.string.common_username)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Text,
                imeAction = ImeAction.Next
            ),
            modifier = Modifier.fillMaxWidth(),
            enabled = !busy
        )

        Spacer(modifier = Modifier.height(12.dp))

        OutlinedTextField(
            value = uiState.password,
            onValueChange = viewModel::onPasswordChanged,
            label = { Text(stringResource(R.string.auth_login_password)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Done
            ),
            keyboardActions = KeyboardActions(
                onDone = { if (!busy) attemptViewModel.signInWithPassword(uiState.username, uiState.password) }
            ),
            modifier = Modifier.fillMaxWidth(),
            enabled = !busy
        )

        val formError = attempt.retryMinutes?.let {
            pluralStringResource(R.plurals.auth_login_too_many_minutes, it, it)
        } ?: (attempt.formError ?: uiState.errorRes)?.let { stringResource(it) }
        formError?.let { text ->
            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = text,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium
            )
        }

        Spacer(modifier = Modifier.height(24.dp))

        Button(
            onClick = { attemptViewModel.signInWithPassword(uiState.username, uiState.password) },
            modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
            enabled = !busy && uiState.username.isNotBlank() && uiState.password.isNotEmpty()
        ) {
            if (busy) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary
                )
                Spacer(modifier = Modifier.width(8.dp))
            }
            Text(stringResource(R.string.auth_sign_in))
        }

        if (uiState.oidcProviders.isNotEmpty()) {
            Spacer(modifier = Modifier.height(24.dp))

            HorizontalDivider()

            Spacer(modifier = Modifier.height(16.dp))

            Text(
                text = stringResource(R.string.auth_or_sign_in_with),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            Spacer(modifier = Modifier.height(12.dp))

            uiState.oidcProviders.forEach { provider ->
                OutlinedButton(
                    onClick = { attemptViewModel.startOidc(context, provider.slug) },
                    modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                    enabled = !busy
                ) {
                    Text(provider.displayName, color = MaterialTheme.colorScheme.onBackground)
                }

                Spacer(modifier = Modifier.height(8.dp))
            }
        }

        Spacer(modifier = Modifier.height(16.dp))

        TextButton(onClick = onSetUpPhone, enabled = !busy) {
            Text(stringResource(R.string.auth_login_set_up_phone))
        }
        TextButton(onClick = { viewModel.serverPageUrl("/recover")?.let { openPage(context, it) } }) {
            Text(stringResource(R.string.auth_login_lost_phone))
        }
        TextButton(onClick = { viewModel.serverPageUrl("/secure-account")?.let { openPage(context, it) } }) {
            Text(stringResource(R.string.auth_login_secure_account))
        }

        Spacer(modifier = Modifier.height(12.dp))

        if (uiState.registrationOpen) {
            TextButton(onClick = onNavigateToRegister) {
                Text(stringResource(R.string.auth_go_to_register))
            }
        }

        if (uiState.registrationCheckFailed) {
            TextButton(onClick = viewModel::onRetryRegistrationCheck) {
                Text(stringResource(R.string.auth_register_check_retry))
            }
        }

        TextButton(
            onClick = viewModel::onChangeServerClicked,
            enabled = !busy
        ) {
            Text(stringResource(R.string.auth_change_server))
        }
    }
}

/** Opens a page of the server's web client in a Custom Tab (recovery, "Secure my account"). */
private fun openPage(context: Context, url: String) {
    runCatching { CustomTabsIntent.Builder().build().launchUrl(context, Uri.parse(url)) }
}
