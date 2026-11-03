package io.wolftown.kaiku.ui.auth.setup

import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.SecureScreen
import io.wolftown.kaiku.ui.shared.currentFragmentActivity
import kotlinx.coroutines.delay

/** How often a waiting setup is checked while the app is in front. */
private const val PENDING_CHECK_MS = 15 * 60 * 1000L

/**
 * L2-A13: the new phone keeps showing when it will be ready, survives restarts
 * and reboots, never falls back to a login form, and signs in by itself (with
 * the screen lock) once the server activated it.
 */
@Composable
fun SetupPendingScreen(
    onSignedIn: () -> Unit,
    onEnded: () -> Unit = {},
    viewModel: SetupPendingViewModel = hiltViewModel(),
) {
    SecureScreen()
    val ui by viewModel.ui.collectAsState()
    val activity = currentFragmentActivity()

    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { viewModel.check() }
    LaunchedEffect(Unit) {
        while (true) {
            delay(PENDING_CHECK_MS)
            viewModel.check()
        }
    }
    LaunchedEffect(ui) {
        when (ui) {
            SetupUi.SigningIn -> activity?.let(viewModel::signIn)
            SetupUi.Done -> onSignedIn()
            else -> Unit
        }
    }
    KeyScreen {
        SetupStatus(ui = ui, onSignIn = { activity?.let(viewModel::signIn) }, onRetry = null, onBack = onEnded)
    }
}
