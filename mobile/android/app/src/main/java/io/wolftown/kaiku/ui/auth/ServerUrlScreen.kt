package io.wolftown.kaiku.ui.auth

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.annotation.StringRes
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.BuildConfig
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ProbeResult
import io.wolftown.kaiku.data.api.ServerProbe
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.InvalidServerAddressException
import io.wolftown.kaiku.domain.normalizeServerAddress
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class ServerUrlUiState(
    val url: String = "",
    /** `@StringRes` of the current error, or null. Never server text. */
    @StringRes val error: Int? = null,
    val isChecking: Boolean = false,
    val connectSuccess: Boolean = false
)

/** Maps a rejected address to copy. */
@StringRes
internal fun serverAddressErrorRes(error: Throwable): Int =
    if (error is InvalidServerAddressException &&
        error.reason == InvalidServerAddressException.Reason.NOT_SECURE
    ) {
        R.string.auth_server_not_secure
    } else {
        R.string.auth_server_invalid_address
    }

@HiltViewModel
class ServerUrlViewModel @Inject constructor(
    private val tokenStorage: TokenStorage,
    private val authRepository: AuthRepository,
    private val serverProbe: ServerProbe
) : ViewModel() {

    // Shows the current server (or the family default) so most people only tap Connect.
    private val _uiState = MutableStateFlow(
        ServerUrlUiState(url = tokenStorage.getServerUrl() ?: BuildConfig.DEFAULT_SERVER_URL)
    )
    val uiState: StateFlow<ServerUrlUiState> = _uiState.asStateFlow()

    fun onUrlChanged(url: String) {
        _uiState.update { it.copy(url = url, error = null) }
    }

    fun onConnectClicked() {
        if (_uiState.value.isChecking) return
        val address = normalizeServerAddress(_uiState.value.url).getOrElse {
            _uiState.update { state -> state.copy(error = serverAddressErrorRes(it)) }
            return
        }

        _uiState.update { it.copy(isChecking = true, error = null) }
        viewModelScope.launch {
            when (serverProbe.probe(address)) {
                is ProbeResult.Ok -> {
                    // A pending password change belongs to the old server; drop it
                    // before the new URL (which bumps the session generation) is saved.
                    authRepository.cancelPasswordChange()
                    tokenStorage.saveServerUrl(address)
                    _uiState.update { it.copy(isChecking = false, connectSuccess = true) }
                }
                ProbeResult.NotKaiku -> _uiState.update {
                    it.copy(isChecking = false, error = R.string.auth_server_not_kaiku)
                }
                ProbeResult.Unreachable -> _uiState.update {
                    it.copy(isChecking = false, error = R.string.auth_server_unreachable)
                }
            }
        }
    }
}

@Composable
fun ServerUrlScreen(
    onConnectSuccess: () -> Unit,
    onScanQrCode: () -> Unit = {},
    viewModel: ServerUrlViewModel = androidx.hilt.navigation.compose.hiltViewModel()
) {
    val uiState by viewModel.uiState.collectAsState()

    LaunchedEffect(uiState.connectSuccess) {
        if (uiState.connectSuccess) {
            onConnectSuccess()
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            text = stringResource(R.string.auth_server_title),
            style = MaterialTheme.typography.headlineLarge,
            color = MaterialTheme.colorScheme.primary
        )

        Spacer(modifier = Modifier.height(8.dp))

        Text(
            text = stringResource(R.string.auth_server_subtitle),
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )

        Spacer(modifier = Modifier.height(32.dp))

        OutlinedTextField(
            value = uiState.url,
            onValueChange = viewModel::onUrlChanged,
            label = { Text(stringResource(R.string.auth_server_address_label)) },
            placeholder = { Text(stringResource(R.string.auth_server_address_hint)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Done
            ),
            keyboardActions = KeyboardActions(
                onDone = { viewModel.onConnectClicked() }
            ),
            isError = uiState.error != null,
            supportingText = uiState.error?.let { errorRes ->
                { Text(stringResource(errorRes)) }
            },
            enabled = !uiState.isChecking,
            modifier = Modifier.fillMaxWidth()
        )

        Spacer(modifier = Modifier.height(24.dp))

        Button(
            onClick = viewModel::onConnectClicked,
            enabled = !uiState.isChecking,
            modifier = Modifier.fillMaxWidth()
        ) {
            if (uiState.isChecking) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary
                )
                Spacer(modifier = Modifier.width(8.dp))
            }
            Text(stringResource(R.string.auth_server_connect))
        }

        Spacer(modifier = Modifier.height(16.dp))
        HorizontalDivider()
        Spacer(modifier = Modifier.height(16.dp))

        OutlinedButton(
            onClick = onScanQrCode,
            enabled = !uiState.isChecking,
            modifier = Modifier.fillMaxWidth()
        ) {
            Text(stringResource(R.string.auth_server_scan_qr))
        }
    }
}
