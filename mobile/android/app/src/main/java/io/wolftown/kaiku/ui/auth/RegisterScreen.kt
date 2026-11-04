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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.ui.auth.setup.SetupEntry
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class RegisterUiState(
    val username: String = "",
    val email: String = "",
    val password: String = "",
    val displayName: String = "",
    val isLoading: Boolean = false,
    /** `@StringRes` of the current error, or null. Server message text is never shown. */
    @StringRes val errorRes: Int? = null,
    /** F8: the account exists; this phone's own setup (bound by the ticket) comes next. */
    val setupEntry: SetupEntry? = null,
)

@HiltViewModel
class RegisterViewModel @Inject constructor(
    private val authRepository: AuthRepository,
    private val enrollmentApi: EnrollmentApi,
    private val tokenStorage: TokenStorage,
) : ViewModel() {

    private val _uiState = MutableStateFlow(RegisterUiState())
    val uiState: StateFlow<RegisterUiState> = _uiState.asStateFlow()

    fun onUsernameChanged(username: String) {
        _uiState.update { it.copy(username = username) }
    }

    fun onEmailChanged(email: String) {
        _uiState.update { it.copy(email = email) }
    }

    fun onPasswordChanged(password: String) {
        _uiState.update { it.copy(password = password) }
    }

    fun onDisplayNameChanged(displayName: String) {
        _uiState.update { it.copy(displayName = displayName) }
    }

    /**
     * The ticket of an account this screen already created (30 min, C27): a retry after a failed
     * issuance only issues again, it never registers the same name twice (Argon part6 #2).
     */
    private var created: Pair<String, String>? = null

    fun onRegisterClicked() {
        val state = _uiState.value
        if (state.isLoading) return
        _uiState.update { it.copy(isLoading = true, errorRes = null) }

        viewModelScope.launch {
            val email = state.email.ifBlank { null }
            val displayName = state.displayName.ifBlank { null }
            val kept = created?.takeIf { it.first == state.username }?.second
            val ticket = kept ?: authRepository.register(
                state.username,
                state.password,
                email,
                displayName
            ).getOrElse { exception ->
                _uiState.update { it.copy(isLoading = false, errorRes = AuthErrorMapper.register(exception)) }
                return@launch
            }.ticket
            created = state.username to ticket
            // F8: no session; the ticket binds this phone's own key (claim without a code).
            try {
                val origin = tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin)
                    ?: throw IllegalStateException("no server")
                val issued = enrollmentApi.issueWithTicket(ticket)
                val entry = SetupEntry(origin, issued.setupToken, SetupEntry.Source.TICKET, ticket = ticket)
                _uiState.update { it.copy(isLoading = false, setupEntry = entry) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // An expired ticket (30 min) is gone with its unconfirmed account: register again.
                if (e is io.wolftown.kaiku.data.api.ApiException && e.normalizedCode == "REGISTRATION_TICKET_INVALID") created = null
                _uiState.update { it.copy(isLoading = false, errorRes = AuthErrorMapper.register(e)) }
            }
        }
    }

    fun clearError() {
        _uiState.update { it.copy(errorRes = null) }
    }

    /** Called once the setup took the entry: coming back here never replays it. */
    fun onSetupStarted() {
        _uiState.update { it.copy(setupEntry = null) }
    }
}

@Composable
fun RegisterScreen(
    onNavigateToLogin: () -> Unit,
    onRegistered: (SetupEntry) -> Unit,
    viewModel: RegisterViewModel = androidx.hilt.navigation.compose.hiltViewModel()
) {
    val uiState by viewModel.uiState.collectAsState()

    LaunchedEffect(uiState.setupEntry) {
        uiState.setupEntry?.let {
            viewModel.onSetupStarted()
            onRegistered(it)
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
            text = stringResource(R.string.auth_register_heading),
            style = MaterialTheme.typography.headlineMedium,
            color = MaterialTheme.colorScheme.primary
        )

        Spacer(modifier = Modifier.height(8.dp))

        Text(
            text = stringResource(R.string.auth_register_subtitle),
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )

        Spacer(modifier = Modifier.height(32.dp))

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
            enabled = !uiState.isLoading
        )

        Spacer(modifier = Modifier.height(12.dp))

        OutlinedTextField(
            value = uiState.email,
            onValueChange = viewModel::onEmailChanged,
            label = { Text(stringResource(R.string.auth_register_email_optional)) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Email,
                imeAction = ImeAction.Next
            ),
            modifier = Modifier.fillMaxWidth(),
            enabled = !uiState.isLoading
        )

        Spacer(modifier = Modifier.height(12.dp))

        OutlinedTextField(
            value = uiState.displayName,
            onValueChange = viewModel::onDisplayNameChanged,
            label = { Text(stringResource(R.string.auth_register_display_name_optional)) },
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
            label = { Text(stringResource(R.string.auth_register_password)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Done
            ),
            keyboardActions = KeyboardActions(
                onDone = { viewModel.onRegisterClicked() }
            ),
            modifier = Modifier.fillMaxWidth(),
            enabled = !uiState.isLoading
        )

        uiState.errorRes?.let { errorRes ->
            Spacer(modifier = Modifier.height(8.dp))

            Text(
                text = stringResource(errorRes),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall
            )
        }

        Spacer(modifier = Modifier.height(24.dp))

        Button(
            onClick = viewModel::onRegisterClicked,
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
            Text(stringResource(R.string.auth_register_submit))
        }

        Spacer(modifier = Modifier.height(12.dp))

        TextButton(onClick = onNavigateToLogin) {
            Text(stringResource(R.string.auth_register_login_link))
        }
    }
}
