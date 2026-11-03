package io.wolftown.kaiku.ui.scan

import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.auth.AccountFlags
import io.wolftown.kaiku.data.auth.OriginPin
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.ScanMode
import io.wolftown.kaiku.domain.ScanPayload
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject

sealed interface ScanUi {
    data object Scanning : ScanUi
    data class Refused(@StringRes val msg: Int) : ScanUi
    data class OpenLogin(val payload: ScanPayload.Login) : ScanUi
    data class OpenSetup(val payload: ScanPayload.Setup) : ScanUi
}

/**
 * One scanner for sign-in and setup codes (L2-A7). Decides from the code alone
 * and the local state, never from the network: another server's code, a
 * sign-in code in setup mode, a phone that is not a key, or an account whose
 * `may_approve` is off are refused before anything is sent.
 */
@HiltViewModel
class ScanViewModel internal constructor(
    private val pinnedOrigin: () -> String?,
    private val mayApprove: () -> Boolean?,
    private val isKeyDevice: () -> Boolean,
) : ViewModel() {

    @Inject
    constructor(pin: OriginPin, flags: AccountFlags, keyDevices: KeyDeviceRepository) : this(
        pinnedOrigin = pin::pinned,
        mayApprove = flags::mayApprove,
        isKeyDevice = { keyDevices.current() is KeyDeviceState.Active },
    ) {
        viewModelScope.launch { flags.refresh() }
    }

    private var mode = ScanMode.SIGN_IN_OR_SETUP
    private val _ui = MutableStateFlow<ScanUi>(ScanUi.Scanning)
    val ui: StateFlow<ScanUi> = _ui.asStateFlow()

    fun setMode(m: ScanMode) {
        mode = m
    }

    fun onCode(raw: String) {
        if (_ui.value != ScanUi.Scanning) return
        _ui.value = decide(KaikuPayloads.pin(KaikuPayloads.parse(raw), pinnedOrigin()))
    }

    private fun decide(p: ScanPayload): ScanUi = when (p) {
        is ScanPayload.Setup -> ScanUi.OpenSetup(p)
        is ScanPayload.Login -> when {
            mode == ScanMode.SETUP_ONLY -> ScanUi.Refused(R.string.scan_sign_in_code_in_setup)
            !isKeyDevice() -> ScanUi.Refused(R.string.scan_not_a_key)
            mayApprove() == false -> ScanUi.Refused(R.string.scan_may_approve_off)
            else -> ScanUi.OpenLogin(p)
        }
        is ScanPayload.OtherServer -> ScanUi.Refused(R.string.scan_other_server)
        ScanPayload.NeedsUpdate -> ScanUi.Refused(R.string.scan_needs_update)
        ScanPayload.OldKaikuCode -> ScanUi.Refused(R.string.scan_old_kaiku_code)
        ScanPayload.NotKaiku -> ScanUi.Refused(R.string.scan_not_kaiku)
    }

    /** After a refusal ("Scan again") or after the screen it opened returned. */
    fun rescan() {
        _ui.value = ScanUi.Scanning
    }
}
