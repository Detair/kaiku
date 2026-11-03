package io.wolftown.kaiku.ui.shared

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.view.WindowManager
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView

/**
 * Screen hardening for approval, setup, code and recovery-code screens (spec B.1):
 * while this is in composition the window is `FLAG_SECURE` (no screenshots, no
 * recents thumbnail, not cast) and the view drops touches that arrive while
 * another app's window covers it (tapjacking). Both are restored on dispose.
 */
@Composable
fun SecureScreen() {
    val view = LocalView.current
    val activity = LocalContext.current.findActivity()
    DisposableEffect(view, activity) {
        val window = activity?.window
        val wasSecure = window != null &&
            (window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE) != 0
        val wasFiltering = view.filterTouchesWhenObscured
        window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        view.filterTouchesWhenObscured = true
        onDispose {
            if (!wasSecure) window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
            view.filterTouchesWhenObscured = wasFiltering
        }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
