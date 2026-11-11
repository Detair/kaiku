package io.wolftown.kaiku.ui.shared

import android.Manifest
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.core.app.ActivityCompat
import androidx.core.app.NotificationManagerCompat
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleResumeEffect
import dagger.hilt.android.lifecycle.HiltViewModel
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.R
import io.wolftown.kaiku.push.NotificationPermissionDecision
import io.wolftown.kaiku.push.NotificationPermissionPolicy
import io.wolftown.kaiku.push.PushRegistrar
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject

/** When the "notifications are off" hint shows (AND-B-12); pure so it can be unit tested. */
object NotificationsOffBannerPolicy {
    /** A closed hint comes back after this long while notifications stay off. */
    const val SNOOZE_MS = 7L * 24 * 60 * 60 * 1000

    fun shouldShow(notificationsEnabled: Boolean, dismissedAtMs: Long?, nowMs: Long): Boolean = when {
        notificationsEnabled -> false
        dismissedAtMs == null -> true
        // A clock set back must not hide the hint for good.
        nowMs < dismissedAtMs -> true
        else -> nowMs - dismissedAtMs >= SNOOZE_MS
    }
}

private const val PUSH_PREFS = "push_prefs"
private const val KEY_ASKED = "notification_permission_asked"
private const val KEY_BANNER_DISMISSED_AT = "notifications_off_banner_dismissed_at"

@HiltViewModel
class NotificationsOffBannerViewModel @Inject constructor(
    @ApplicationContext private val context: Context,
    private val pushRegistrar: PushRegistrar,
) : ViewModel() {

    private val prefs = context.getSharedPreferences(PUSH_PREFS, Context.MODE_PRIVATE)

    private val _visible = MutableStateFlow(false)
    val visible: StateFlow<Boolean> = _visible.asStateFlow()

    private var wasEnabled: Boolean? = null

    /** Re-reads the system setting; runs on every resume (the user may come back from settings). */
    fun refresh() {
        val enabled = NotificationManagerCompat.from(context).areNotificationsEnabled()
        // Turned on outside the app (system settings): let push register now.
        if (enabled && wasEnabled == false) pushRegistrar.onPermissionChanged()
        wasEnabled = enabled
        val dismissedAt = prefs.getLong(KEY_BANNER_DISMISSED_AT, -1L).takeIf { it >= 0 }
        _visible.value = NotificationsOffBannerPolicy.shouldShow(enabled, dismissedAt, System.currentTimeMillis())
    }

    fun dismiss() {
        prefs.edit().putLong(KEY_BANNER_DISMISSED_AT, System.currentTimeMillis()).apply()
        _visible.value = false
    }

    fun askedBefore(): Boolean = prefs.getBoolean(KEY_ASKED, false)

    fun markAsked() {
        prefs.edit().putBoolean(KEY_ASKED, true).apply()
    }

    fun onGranted() {
        pushRegistrar.onPermissionChanged()
        refresh()
    }
}

/**
 * A lasting hint while notifications are off (AND-B-12): "Turn on" asks for the
 * permission when Android still allows it, otherwise opens the app's
 * notification settings; "Close" hides it for [NotificationsOffBannerPolicy.SNOOZE_MS].
 */
@Composable
fun NotificationsOffBanner(
    modifier: Modifier = Modifier,
    viewModel: NotificationsOffBannerViewModel = hiltViewModel(),
) {
    val context = LocalContext.current
    val visible by viewModel.visible.collectAsState()

    LifecycleResumeEffect(viewModel) {
        viewModel.refresh()
        onPauseOrDispose { }
    }

    val launcher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted -> if (granted) viewModel.onGranted() else viewModel.refresh() }

    if (!visible) return

    fun turnOn() {
        val activity = context.findActivity()
        val decision = NotificationPermissionPolicy.decide(
            sdkInt = Build.VERSION.SDK_INT,
            notificationsEnabled = false,
            askedBefore = viewModel.askedBefore(),
            shouldShowRationale = Build.VERSION.SDK_INT >= 33 && activity != null &&
                ActivityCompat.shouldShowRequestPermissionRationale(
                    activity, Manifest.permission.POST_NOTIFICATIONS
                ),
        )
        if (decision == NotificationPermissionDecision.ASK_WITH_RATIONALE) {
            viewModel.markAsked()
            launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else {
            openAppNotificationSettings(context)
        }
    }

    Surface(
        modifier = modifier
            .fillMaxWidth()
            .semantics { liveRegion = LiveRegionMode.Polite },
        color = MaterialTheme.colorScheme.secondaryContainer,
        contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
    ) {
        Column(modifier = Modifier.padding(start = 16.dp, end = 8.dp, top = 8.dp)) {
            Text(
                text = stringResource(R.string.notifications_off_banner_text),
                style = MaterialTheme.typography.bodyMedium,
            )
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
            ) {
                TextButton(onClick = viewModel::dismiss) {
                    Text(
                        stringResource(R.string.notifications_off_banner_close),
                        color = MaterialTheme.colorScheme.onSecondaryContainer,
                    )
                }
                TextButton(onClick = ::turnOn) {
                    Text(
                        stringResource(R.string.notifications_off_banner_turn_on),
                        color = MaterialTheme.colorScheme.primary,
                    )
                }
            }
        }
    }
}

/** Opens Android's notification settings for this app; does nothing if there is none. */
fun openAppNotificationSettings(context: Context) {
    val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
        .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
        context.startActivity(intent)
    } catch (_: ActivityNotFoundException) {
        // No settings screen on this device.
    }
}

private tailrec fun Context.findActivity(): android.app.Activity? = when (this) {
    is android.app.Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
