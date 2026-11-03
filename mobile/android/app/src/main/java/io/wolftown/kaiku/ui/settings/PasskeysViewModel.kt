package io.wolftown.kaiku.ui.settings

import android.app.Activity
import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.PasskeyApi
import io.wolftown.kaiku.data.api.PasskeyApiException
import io.wolftown.kaiku.data.auth.DeviceStepUp
import io.wolftown.kaiku.data.auth.PasskeyCredentials
import io.wolftown.kaiku.data.auth.PasskeyFailure
import io.wolftown.kaiku.data.auth.PasskeyStepUp
import io.wolftown.kaiku.data.auth.StepUpAvailability
import io.wolftown.kaiku.data.auth.StepUpProvider
import io.wolftown.kaiku.data.auth.ceremony
import io.wolftown.kaiku.data.auth.publicKeyJson
import io.wolftown.kaiku.domain.model.Passkey
import io.wolftown.kaiku.ui.auth.passkeyErrorRes
import java.util.Optional
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class PasskeysUiState(
    val loading: Boolean = true,
    val passkeys: List<Passkey> = emptyList(),
    /** Add is offered with a key device's step-up (the first passkey, DC1) or an existing passkey. */
    val canAdd: Boolean = false,
    val busy: Boolean = false,
    @StringRes val errorRes: Int? = null,
    /** With `passkeys_error_ready_at`: the ready time to show. */
    val readyAt: String? = null,
)

/**
 * Passkeys in Settings (C29): list (pending rows show their ready time),
 * add after a step-up for `passkey.add`, remove after a passkey step-up for
 * that passkey, then signal. Rename is web/desktop only.
 */
@HiltViewModel
class PasskeysViewModel @Inject constructor(
    private val api: PasskeyApi,
    private val credentials: PasskeyCredentials,
    private val passkeyStepUp: PasskeyStepUp,
    @DeviceStepUp private val deviceStepUp: Optional<StepUpProvider>,
) : ViewModel() {
    private val _state = MutableStateFlow(PasskeysUiState())
    val state: StateFlow<PasskeysUiState> = _state.asStateFlow()

    init {
        reload()
    }

    fun reload() {
        viewModelScope.launch {
            try {
                val list = api.list().passkeys.filter { it.isActive || it.isPending }
                val canAdd = addAvailable(list)
                _state.update {
                    it.copy(
                        loading = false,
                        passkeys = list,
                        canAdd = canAdd,
                    )
                }
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                _state.update { it.copy(loading = false, errorRes = R.string.passkeys_error_failed) }
            }
        }
    }

    /**
     * L2 (Fable m5): a bound provider that knows whether a step-up can work here
     * (full session, and the key or a passkey) decides; otherwise as before.
     */
    private suspend fun addAvailable(list: List<Passkey>): Boolean {
        val hasPasskey = list.any(Passkey::isActive)
        val signal = deviceStepUp.orElse(null) as? StepUpAvailability
            ?: return deviceStepUp.isPresent || hasPasskey
        return signal.available(hasPasskey)
    }

    /** The step-up for adding: the key device's own when bound, else a passkey. */
    private fun addStepUp(): StepUpProvider? = when {
        deviceStepUp.isPresent -> deviceStepUp.get()
        _state.value.passkeys.any(Passkey::isActive) -> passkeyStepUp
        else -> null
    }

    fun add(activity: Activity, label: String) {
        val provider = addStepUp() ?: return
        run {
            provider.stepUp(activity, "passkey.add", "").getOrThrow()
            val start = api.registerStart(label.take(64).ifBlank { null })
            val credential = ceremony { credentials.create(activity, publicKeyJson(start)) }
            api.registerFinish(start.ceremonyId, credential)
        }
    }

    fun remove(activity: Activity, passkey: Passkey) {
        run {
            passkeyStepUp.stepUp(activity, "passkey.remove", passkey.id).getOrThrow()
            val removed = api.delete(passkey.id)
            credentials.signalRemoved(
                removed.rpId,
                removed.userHandle,
                removed.credentialId,
                removed.remainingCredentialIds,
            )
        }
    }

    private fun run(action: suspend () -> Unit) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true, errorRes = null, readyAt = null) }
        viewModelScope.launch {
            try {
                action()
                _state.update { it.copy(busy = false) }
                reload()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                val res = when (e) {
                    PasskeyFailure.Cancelled -> null
                    is PasskeyApiException -> passkeyErrorRes(PasskeyFailure.Server(e.normalizedCode ?: "PASSKEY_FAILED"))
                    else -> passkeyErrorRes(e)
                }
                _state.update {
                    it.copy(busy = false, errorRes = res, readyAt = (e as? PasskeyFailure.Pending)?.readyAt)
                }
                // A passkey the server no longer knows leaves the list.
                if (e is PasskeyFailure.Removed) reload()
            }
        }
    }
}
