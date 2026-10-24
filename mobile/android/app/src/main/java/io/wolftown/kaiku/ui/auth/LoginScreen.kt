package io.wolftown.kaiku.ui.auth

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.OidcProvider

@Composable
fun LoginScreen(
    onNavigateToRegister: () -> Unit,
    onLoginSuccess: () -> Unit,
    onPasswordChangeRequired: () -> Unit = {},
    oidcProviders: List<OidcProvider> = emptyList(),
    onOidcLogin: (OidcProvider) -> Unit = {},
    viewModel: LoginViewModel = hiltViewModel()
) {
    val context = LocalContext.current
    val uiState by viewModel.uiState.collectAsState()

    LaunchedEffect(uiState.loginSuccess) {
        if (uiState.loginSuccess) {
            onLoginSuccess()
        }
    }

    LaunchedEffect(uiState.passwordChangeRequired) {
        if (uiState.passwordChangeRequired) {
            viewModel.onPasswordChangeNavigated()
            onPasswordChangeRequired()
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

        Spacer(modifier = Modifier.height(32.dp))

        OutlinedTextField(
            value = uiState.username,
            onValueChange = viewModel::onUsernameChanged,
            label = { Text(stringResource(R.string.auth_username)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Text,
                imeAction = ImeAction.Next
            ),
            modifier = Modifier.fillMaxWidth(),
            enabled = !uiState.isLoading
        )

        Spacer(modifier = Modifier.height(12.dp))

        OutlinedTextField(
            value = uiState.password,
            onValueChange = viewModel::onPasswordChanged,
            label = { Text(stringResource(R.string.auth_password)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = if (uiState.mfaRequired) ImeAction.Next else ImeAction.Done
            ),
            keyboardActions = KeyboardActions(
                onDone = {
                    if (!uiState.mfaRequired) viewModel.onLoginClicked()
                }
            ),
            modifier = Modifier.fillMaxWidth(),
            enabled = !uiState.isLoading
        )

        if (uiState.mfaRequired) {
            Spacer(modifier = Modifier.height(12.dp))

            OutlinedTextField(
                value = uiState.mfaCode,
                onValueChange = viewModel::onMfaCodeChanged,
                label = { Text(stringResource(R.string.auth_mfa_code)) },
                singleLine = true,
                keyboardOptions = KeyboardOptions(
                    keyboardType = KeyboardType.Number,
                    imeAction = ImeAction.Done
                ),
                keyboardActions = KeyboardActions(
                    onDone = { viewModel.onLoginClicked() }
                ),
                modifier = Modifier.fillMaxWidth(),
                enabled = !uiState.isLoading
            )
        }

        uiState.error?.let { errorRes ->
            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = stringResource(errorRes),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall
            )
        }

        Spacer(modifier = Modifier.height(24.dp))

        Button(
            onClick = viewModel::onLoginClicked,
            modifier = Modifier.fillMaxWidth(),
            enabled = !uiState.isLoading
        ) {
            if (uiState.isLoading) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary
                )
                Spacer(modifier = Modifier.width(8.dp))
            }
            Text(stringResource(R.string.auth_sign_in))
        }

        Spacer(modifier = Modifier.height(12.dp))

        TextButton(onClick = onNavigateToRegister) {
            Text(stringResource(R.string.auth_go_to_register))
        }

        if (oidcProviders.isNotEmpty()) {
            Spacer(modifier = Modifier.height(24.dp))

            HorizontalDivider()

            Spacer(modifier = Modifier.height(16.dp))

            Text(
                text = stringResource(R.string.auth_or_sign_in_with),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )

            Spacer(modifier = Modifier.height(12.dp))

            oidcProviders.forEach { provider ->
                OutlinedButton(
                    onClick = {
                        viewModel.launchOidcLogin(context, provider.slug)
                        onOidcLogin(provider)
                    },
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !uiState.isLoading
                ) {
                    Text(provider.displayName)
                }

                Spacer(modifier = Modifier.height(8.dp))
            }
        }
    }
}
