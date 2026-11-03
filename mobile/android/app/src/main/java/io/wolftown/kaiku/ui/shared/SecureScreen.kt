package io.wolftown.kaiku.ui.shared

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.view.WindowManager
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import java.util.WeakHashMap

/**
 * Screen hardening for approval, setup, code and recovery-code screens (spec B.1):
 * while this is in composition the window is `FLAG_SECURE` (no screenshots, no
 * recents thumbnail, not cast) and the view drops touches that arrive while
 * another app's window covers it (tapjacking). Protection is reference-counted
 * per window and per view ([SecureScreens]): during an animated transition
 * between two protected screens the outgoing one never lifts it, and the
 * original flags come back only when the last protected screen leaves.
 */
@Composable
fun SecureScreen() {
    val view = LocalView.current
    val activity = LocalContext.current.findActivity()
    DisposableEffect(view, activity) {
        val window = activity?.window
        val setSecure: (Boolean) -> Unit = { on ->
            if (on) window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
            else window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
        val setFiltering: (Boolean) -> Unit = { view.filterTouchesWhenObscured = it }
        SecureScreens.acquire(
            window, view,
            isSecure = { window != null && (window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE) != 0 },
            setSecure = setSecure,
            isFiltering = { view.filterTouchesWhenObscured },
            setFiltering = setFiltering,
        )
        onDispose { SecureScreens.release(window, view, setSecure, setFiltering) }
    }
}

/**
 * Reference counts behind [SecureScreen]: one for the window's `FLAG_SECURE`, one
 * for the view's touch filtering (a dialog has its own view on the same window).
 * The first acquire records the original value, the last release restores it.
 * Main thread only, like the composition that calls it.
 */
internal object SecureScreens {
    private class Count(val original: Boolean, var n: Int)

    private val windows = WeakHashMap<Any, Count>()
    private val views = WeakHashMap<Any, Count>()

    fun acquire(
        window: Any?,
        view: Any,
        isSecure: () -> Boolean,
        setSecure: (Boolean) -> Unit,
        isFiltering: () -> Boolean,
        setFiltering: (Boolean) -> Unit,
    ) {
        if (window != null) take(windows, window, isSecure, setSecure)
        take(views, view, isFiltering, setFiltering)
    }

    fun release(window: Any?, view: Any, setSecure: (Boolean) -> Unit, setFiltering: (Boolean) -> Unit) {
        if (window != null) give(windows, window, setSecure)
        give(views, view, setFiltering)
    }

    fun activeCount(window: Any): Int = windows[window]?.n ?: 0

    private fun take(map: WeakHashMap<Any, Count>, key: Any, get: () -> Boolean, set: (Boolean) -> Unit) {
        val c = map[key]
        if (c == null) map[key] = Count(get(), 1) else c.n++
        set(true)
    }

    private fun give(map: WeakHashMap<Any, Count>, key: Any, set: (Boolean) -> Unit) {
        val c = map[key] ?: return
        if (--c.n > 0) return
        map.remove(key)
        set(c.original)
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
