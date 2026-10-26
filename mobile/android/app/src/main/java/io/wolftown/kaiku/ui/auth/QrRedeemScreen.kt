package io.wolftown.kaiku.ui.auth

import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R

@Composable
fun QrRedeemScreen(
    serverUrl: String,
    token: String,
    onSuccess: () -> Unit,
    onError: () -> Unit
) {
    val viewModel: QrRedeemViewModel = hiltViewModel()
    val state by viewModel.uiState.collectAsState()

    LaunchedEffect(Unit) { viewModel.start(serverUrl, token) }
    LaunchedEffect(state.succeeded) { if (state.succeeded) onSuccess() }

    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center
    ) {
        if (state.isLoading || state.succeeded) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                CircularProgressIndicator()
                Spacer(modifier = Modifier.height(16.dp))
                Text(
                    stringResource(R.string.qr_redeem_signing_in),
                    style = MaterialTheme.typography.bodyLarge
                )
            }
        } else {
            // Not loading and not signed in: always say why, even if no specific error was stored.
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(
                    text = stringResource(state.errorRes ?: R.string.qr_redeem_failed),
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyLarge
                )
                Spacer(modifier = Modifier.height(16.dp))
                Button(onClick = onError) {
                    Text(stringResource(R.string.common_back))
                }
            }
        }
    }
}
