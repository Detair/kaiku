package io.wolftown.kaiku.ui.shared

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.core.app.ActivityCompat
import androidx.core.app.NotificationManagerCompat
import io.wolftown.kaiku.R
import io.wolftown.kaiku.push.NotificationPermissionDecision
import io.wolftown.kaiku.push.NotificationPermissionPolicy

private const val PUSH_PREFS = "push_prefs"
private const val KEY_ASKED = "notification_permission_asked"

/**
 * Asks once, after sign-in, for permission to show notifications: a short
 * explanation first, then the system request. [onGranted] runs when
 * notifications are (or become) allowed.
 */
@Composable
fun NotificationPermissionPrompt(onGranted: () -> Unit) {
    val context = LocalContext.current
    val prefs = remember { context.getSharedPreferences(PUSH_PREFS, Context.MODE_PRIVATE) }
    val askedBefore = remember { prefs.getBoolean(KEY_ASKED, false) }
    val decision = remember {
        val activity = context.findActivity()
        NotificationPermissionPolicy.decide(
            sdkInt = Build.VERSION.SDK_INT,
            notificationsEnabled = NotificationManagerCompat.from(context).areNotificationsEnabled(),
            askedBefore = askedBefore,
            shouldShowRationale = Build.VERSION.SDK_INT >= 33 && activity != null &&
                ActivityCompat.shouldShowRequestPermissionRationale(
                    activity, Manifest.permission.POST_NOTIFICATIONS
                ),
        )
    }
    var showDialog by remember {
        mutableStateOf(decision == NotificationPermissionDecision.ASK_WITH_RATIONALE)
    }

    val launcher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted -> if (granted) onGranted() }

    // Already allowed: let the caller (push registration) proceed.
    LaunchedEffect(decision) {
        if (decision == NotificationPermissionDecision.GRANTED) onGranted()
    }

    if (!showDialog) return

    fun markAsked() {
        prefs.edit().putBoolean(KEY_ASKED, true).apply()
        showDialog = false
    }

    AlertDialog(
        onDismissRequest = { markAsked() },
        text = { Text(stringResource(R.string.push_permission_rationale)) },
        confirmButton = {
            TextButton(onClick = {
                markAsked()
                launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }) { Text(stringResource(R.string.push_permission_continue)) }
        },
        dismissButton = {
            TextButton(onClick = { markAsked() }) {
                Text(stringResource(R.string.push_permission_not_now))
            }
        },
    )
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
