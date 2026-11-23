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
import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.domain.model.MeSecurity
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

/**
 * The chat-only banner's text from the security overview: "Add a phone" only when the account
 * has a key phone and `may_approve` is on (the button exists there), "sign in with a passkey" only
 * when it has one, otherwise the admin. Without the overview it names no way.
 */
fun probationBannerText(sec: MeSecurity?): Int {
    if (sec == null) return R.string.auth_probation_banner_neutral
    val addPhone = sec.keyDevices > 0 && sec.mayApprove
    val passkey = sec.passkeys > 0
    return when {
        addPhone && passkey -> R.string.auth_probation_banner
        addPhone -> R.string.auth_probation_banner_add_phone
        passkey -> R.string.auth_probation_banner_passkey
        else -> R.string.auth_probation_banner_ask_admin
    }
}

/** Whether the home screen shows the chat-only banner (from `GET /auth/me`). */
@HiltViewModel
class ProbationBannerViewModel @Inject constructor(
    private val authApi: AuthApi,
    private val securityApi: SecurityApi,
) : ViewModel() {
    private val _visible = MutableStateFlow(false)
    val visible: StateFlow<Boolean> = _visible.asStateFlow()

    /**
     * The banner text ([probationBannerText]). "Set up this PC" is for computers, so it is never
     * named on a phone.
     */
    private val _text = MutableStateFlow(R.string.auth_probation_banner_neutral)
    val text: StateFlow<Int> = _text.asStateFlow()

    private var job: kotlinx.coroutines.Job? = null

    init {
        refresh()
    }

    /** Read again at every resume of home: a passing network error does not hide it for good. */
    fun refresh() {
        job?.cancel()
        job = viewModelScope.launch {
            try {
                val me = authApi.getMe()
                if (isProbation(me)) {
                    val sec = try {
                        securityApi.meSecurity()
                    } catch (e: CancellationException) {
                        throw e
                    } catch (_: Exception) {
                        null
                    }
                    _text.value = probationBannerText(sec)
                }
                _visible.value = isProbation(me)
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                // Unknown keeps what was known (nothing at first): the banner never claims what it could not read.
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
    val text by viewModel.text.collectAsState()
    androidx.lifecycle.compose.LifecycleEventEffect(androidx.lifecycle.Lifecycle.Event.ON_RESUME) { viewModel.refresh() }
    if (!visible) return
    Surface(color = MaterialTheme.colorScheme.surfaceVariant, modifier = modifier.fillMaxWidth()) {
        KeyNotice(
            text = stringResource(text),
            warning = false,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
        )
    }
}
