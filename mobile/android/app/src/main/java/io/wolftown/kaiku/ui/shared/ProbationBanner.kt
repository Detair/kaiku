package io.wolftown.kaiku.ui.shared

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.AuthApi
import io.wolftown.kaiku.domain.model.User
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/**
 * B.4/CC4: a session is full only when a passkey or the key created it; a
 * profile without `session`, or with any other assurance, is probation.
 */
fun isProbation(me: User): Boolean = me.session?.assurance != "full"

/** Whether the home screen shows the chat-only banner (from `GET /auth/me`). */
@HiltViewModel
class ProbationBannerViewModel @Inject constructor(private val authApi: AuthApi) : ViewModel() {
    private val _visible = MutableStateFlow(false)
    val visible: StateFlow<Boolean> = _visible.asStateFlow()

    init {
        viewModelScope.launch {
            _visible.value = try {
                isProbation(authApi.getMe())
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                // Unknown is not shown: the banner never claims what it could not read.
                false
            }
        }
    }
}

/**
 * The probation banner on home (Task B14): "Signed in for chat only on this
 * phone", with the two ways to a full session. Icon and text, never colour alone.
 */
@Composable
fun ProbationBanner(modifier: Modifier = Modifier, viewModel: ProbationBannerViewModel = hiltViewModel()) {
    val visible by viewModel.visible.collectAsState()
    if (!visible) return
    Surface(color = MaterialTheme.colorScheme.surfaceVariant, modifier = modifier.fillMaxWidth()) {
        KeyNotice(
            text = stringResource(R.string.auth_probation_banner),
            warning = false,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
        )
    }
}
