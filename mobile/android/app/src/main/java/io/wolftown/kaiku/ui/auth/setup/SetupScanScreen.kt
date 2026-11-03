package io.wolftown.kaiku.ui.auth.setup

import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.res.stringResource
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.UnsuitableReason
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.domain.ScanMode
import io.wolftown.kaiku.ui.scan.ScanScreen
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen

/**
 * L2-A1: scan a setup code, then set up this phone. A sign-in code is refused
 * here ("To set up this phone you need a setup code"). The session it ends
 * with is full (device-key sign-in, B.4). The setup state lives in the
 * ViewModel (not in saved state, so no token is persisted): a rotation keeps
 * the flow, and "Back" from an ended setup returns to the scanner for a new code.
 * The camera opens only once the phone can be a key (C26 suitability: screen
 * lock and hardware key store before any scan).
 */
@Composable
fun SetupScanScreen(onSignedIn: () -> Unit, onBack: (() -> Unit)?, flow: SetupFlowViewModel = hiltViewModel()) {
    val started by flow.startedFlow.collectAsState()
    val ready by flow.scanReady.collectAsState()
    LaunchedEffect(Unit) { if (flow.scanReady.value == null) flow.checkBeforeScan() }
    // Back from the screen-lock settings: check again.
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        if ((flow.scanReady.value as? UvDecision.Unsuitable)?.reason == UnsuitableReason.NO_SCREEN_LOCK) flow.checkBeforeScan()
    }
    val r = ready
    when {
        started -> SetupFlowScreen(entry = null, onSignedIn = onSignedIn, onBack = flow::reset, viewModel = flow)
        r == null -> KeyScreen { CircularProgressIndicator() }
        r is UvDecision.Unsuitable && r.reason == UnsuitableReason.NO_SCREEN_LOCK ->
            ScreenLockGuide(onCheck = flow::checkBeforeScan, onCancel = { onBack?.invoke() })
        r is UvDecision.Unsuitable -> KeyScreen {
            KeyNotice(stringResource(R.string.setup_cannot_be_key), warning = true)
            if (onBack != null) KeyPrimaryButton(stringResource(R.string.common_back), onClick = onBack)
        }
        else -> ScanScreen(
            mode = ScanMode.SETUP_ONLY,
            onLogin = {},
            onSetup = { flow.start(SetupEntry(it.origin, it.tokenB64u, SetupEntry.Source.SCAN)) },
            onBack = { onBack?.invoke() },
        )
    }
}
