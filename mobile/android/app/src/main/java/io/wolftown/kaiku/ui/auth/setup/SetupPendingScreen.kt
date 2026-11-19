package io.wolftown.kaiku.ui.auth.setup

import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.compose.ui.res.stringResource
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
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
    /** Shown inside the setup screen: a setup link opened meanwhile was not used. */
    linkIgnored: Boolean = false,
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
    // Back while a claim waits for its code asks first, as on the setup screen. "Stop setup" then
    // leaves as Back did before; the claim stays and is checked again at the next start. While the
    // final sign-in runs, Back does nothing.
    var askLeave by remember { mutableStateOf(false) }
    var leaving by remember { mutableStateOf(false) }
    val backDispatcher = LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher
    val back = pendingBackAction(ui)
    val asks = back == SetupBack.ASK
    BackHandler(enabled = back != SetupBack.LEAVE && !leaving) { if (asks) askLeave = true }
    if (askLeave && asks) {
        SetupLeaveDialog(
            atCode = true,
            onStay = { askLeave = false },
            onStop = {
                askLeave = false
                leaving = true
            },
        )
    }
    LaunchedEffect(leaving) {
        if (leaving) {
            backDispatcher?.onBackPressed()
            leaving = false
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
        if (linkIgnored) KeyNotice(stringResource(R.string.setup_link_ignored), warning = false)
        when (ui) {
            SetupUi.Opening -> {
                CircularProgressIndicator()
                KeyBody(stringResource(R.string.setup_checking))
            }
            else -> SetupStatus(ui = ui, onSignIn = { activity?.let(viewModel::signIn) }, onRetry = viewModel::check, onBack = onEnded)
        }
    }
}
