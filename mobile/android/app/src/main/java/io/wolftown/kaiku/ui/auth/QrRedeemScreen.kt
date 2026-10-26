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
    var isLoading by remember { mutableStateOf(true) }

    val viewModel: QrRedeemViewModel = hiltViewModel()
    val errorRes by viewModel.errorRes.collectAsState()

    LaunchedEffect(Unit) {
        if (viewModel.redeem(serverUrl, token)) {
            onSuccess()
        } else {
            isLoading = false
        }
    }

    Box(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center
    ) {
        if (isLoading) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                CircularProgressIndicator()
                Spacer(modifier = Modifier.height(16.dp))
                Text(
                    stringResource(R.string.qr_redeem_signing_in),
                    style = MaterialTheme.typography.bodyLarge
                )
            }
        } else {
            errorRes?.let { message ->
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        text = stringResource(message),
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
}
