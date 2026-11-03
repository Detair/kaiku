package io.wolftown.kaiku.ui.auth.setup

import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.domain.ScanMode
import io.wolftown.kaiku.ui.scan.ScanScreen

/**
 * L2-A1: scan a setup code, then set up this phone. A sign-in code is refused
 * here ("To set up this phone you need a setup code"). The session it ends
 * with is full (device-key sign-in, B.4). The setup state lives in the
 * ViewModel (not in saved state, so no token is persisted): a rotation keeps
 * the flow, and "Back" from an ended setup returns to the scanner for a new code.
 */
@Composable
fun SetupScanScreen(onSignedIn: () -> Unit, onBack: (() -> Unit)?, flow: SetupFlowViewModel = hiltViewModel()) {
    val started by flow.startedFlow.collectAsState()
    if (!started) {
        ScanScreen(
            mode = ScanMode.SETUP_ONLY,
            onLogin = {},
            onSetup = { flow.start(SetupEntry(it.origin, it.tokenB64u, SetupEntry.Source.SCAN)) },
            onBack = { onBack?.invoke() },
        )
    } else {
        SetupFlowScreen(entry = null, onSignedIn = onSignedIn, onBack = flow::reset, viewModel = flow)
    }
}
