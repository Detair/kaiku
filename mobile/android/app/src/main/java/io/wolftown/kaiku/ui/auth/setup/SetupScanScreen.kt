package io.wolftown.kaiku.ui.auth.setup

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import io.wolftown.kaiku.domain.ScanMode
import io.wolftown.kaiku.ui.scan.ScanScreen

/**
 * L2-A1: scan a setup code, then set up this phone. A sign-in code is refused
 * here ("To set up this phone you need a setup code"). The session it ends
 * with is full (device-key sign-in, B.4).
 */
@Composable
fun SetupScanScreen(onSignedIn: () -> Unit, onBack: (() -> Unit)?) {
    // In memory only: the setup token never goes into saved state.
    var entry by remember { mutableStateOf<SetupEntry?>(null) }
    val current = entry
    if (current == null) {
        ScanScreen(
            mode = ScanMode.SETUP_ONLY,
            onLogin = {},
            onSetup = { entry = SetupEntry(it.origin, it.tokenB64u, SetupEntry.Source.SCAN) },
            onBack = { onBack?.invoke() },
        )
    } else {
        SetupFlowScreen(entry = current, onSignedIn = onSignedIn, onBack = { entry = null })
    }
}
