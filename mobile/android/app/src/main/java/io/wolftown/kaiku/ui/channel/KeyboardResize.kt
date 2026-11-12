package io.wolftown.kaiku.ui.channel

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.view.WindowManager
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.ui.platform.LocalView
import java.util.WeakHashMap

/**
 * Reference count of chat screens that need `adjustResize` in one window
 * (AND-B-2, review W1). The first one remembers the window's own mode, the
 * last one to leave restores it. Overlapping screens (chat A fading out while
 * chat B fades in after a notification tap) therefore keep `adjustResize`
 * until the last chat is gone, and never "restore" a mode another chat set.
 */
internal class SoftInputModeLease {
    private var holders = 0
    private var original: Int? = null

    /** Called when a chat appears; returns the mode to apply. [current] is the window's mode now. */
    fun acquire(current: Int): Int {
        if (holders++ == 0) original = current
        // Keep the state bits (show/hide on focus), replace only the adjust bits.
        return (current and ADJUST_MASK.inv()) or ADJUST_RESIZE
    }

    /** Called when a chat leaves; returns the mode to restore, or null while other chats remain. */
    fun release(): Int? {
        if (holders == 0) return null
        holders--
        if (holders > 0) return null
        return original.also { original = null }
    }

    companion object {
        const val ADJUST_MASK = WindowManager.LayoutParams.SOFT_INPUT_MASK_ADJUST

        @Suppress("DEPRECATION") // Edge-to-edge still needs it on Android < 11 to get IME insets.
        const val ADJUST_RESIZE = WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE
    }
}

/** One lease per activity window; weak so a destroyed activity is not kept. Main thread only. */
private val leases = WeakHashMap<Activity, SoftInputModeLease>()

/**
 * While shown, the window resizes for the keyboard (`adjustResize`) instead of
 * panning, so the IME insets reach `imePadding` on every Android version and the
 * title bar stays put. When the last chat leaves, the window's own mode is
 * restored, so screens that rely on panning are unchanged.
 */
@Composable
internal fun KeyboardResizesWindow() {
    val view = LocalView.current
    DisposableEffect(view) {
        val activity = view.context.findActivity()
        val window = activity?.window
        if (activity == null || window == null) return@DisposableEffect onDispose { }
        val lease = leases.getOrPut(activity) { SoftInputModeLease() }
        window.setSoftInputMode(lease.acquire(window.attributes.softInputMode))
        onDispose {
            lease.release()?.let { window.setSoftInputMode(it) }
        }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
