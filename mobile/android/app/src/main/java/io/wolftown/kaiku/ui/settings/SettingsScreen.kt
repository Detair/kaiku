package io.wolftown.kaiku.ui.settings

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R

private const val NTFY_HELP_URL = "https://docs.ntfy.sh/subscribe/phone/"

private fun openNotificationSettings(context: Context) {
    val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
        .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
        context.startActivity(intent)
    } catch (_: ActivityNotFoundException) {
        // No settings screen to show; the status line still explains the state.
    }
}

private fun openNtfyHelp(context: Context) {
    try {
        CustomTabsIntent.Builder().build().launchUrl(context, Uri.parse(NTFY_HELP_URL))
    } catch (_: ActivityNotFoundException) {
        // No browser installed.
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    appVersion: String,
    onNavigateBack: () -> Unit,
    onLogout: () -> Unit,
    viewModel: SettingsViewModel = hiltViewModel()
) {
    val user by viewModel.user.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val serverUrl = viewModel.serverUrl
    val notificationState by viewModel.notificationState.collectAsState()
    val context = LocalContext.current

    var showLogoutDialog by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        viewModel.reloadPushState()
    }

    LaunchedEffect(Unit) {
        viewModel.logoutComplete.collect { onLogout() }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings_modal_title)) },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.common_back)
                        )
                    }
                }
            )
        }
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
                .padding(horizontal = 16.dp)
        ) {
            // Account section
            Text(
                text = stringResource(R.string.settings_section_account),
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(vertical = 12.dp)
            )

            if (isLoading) {
                CircularProgressIndicator(
                    modifier = Modifier
                        .align(Alignment.CenterHorizontally)
                        .padding(16.dp),
                    strokeWidth = 2.dp
                )
            } else if (user != null) {
                SettingsRow(label = stringResource(R.string.common_username), value = user!!.username)
                SettingsRow(label = stringResource(R.string.settings_display_name), value = user!!.displayName)
                SettingsRow(
                    label = stringResource(R.string.settings_status_label),
                    value = stringResource(statusLabelRes(user!!.status))
                )
            }

            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))

            // Server section
            Text(
                text = stringResource(R.string.settings_section_server),
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(vertical = 12.dp)
            )

            SettingsRow(
                label = stringResource(R.string.settings_server_url),
                value = serverUrl ?: stringResource(R.string.settings_server_url_not_configured)
            )

            // No QR sign-in here: switching accounts needs signing out first
            // (AuthRepository refuses to replace a running session).

            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))

            NotificationSettingsSection(
                state = notificationState,
                onPreferUnifiedPush = viewModel::setPreferUnifiedPush,
                onPickDistributor = viewModel::pickDistributor,
                onOpenSystemSettings = { openNotificationSettings(context) },
                onInstallNtfy = { openNtfyHelp(context) },
            )

            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))

            // About section
            Text(
                text = stringResource(R.string.settings_section_about),
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(vertical = 12.dp)
            )

            SettingsRow(label = stringResource(R.string.settings_app_version), value = appVersion)

            Spacer(modifier = Modifier.weight(1f))

            // Logout button
            Button(
                onClick = { showLogoutDialog = true },
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(vertical = 16.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.error
                )
            ) {
                Text(stringResource(R.string.home_user_panel_logout))
            }
        }
    }

    if (showLogoutDialog) {
        AlertDialog(
            onDismissRequest = { showLogoutDialog = false },
            title = { Text(stringResource(R.string.home_user_panel_logout)) },
            text = { Text(stringResource(R.string.settings_logout_confirm)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        showLogoutDialog = false
                        viewModel.logout()
                    }
                ) {
                    Text(stringResource(R.string.home_user_panel_logout), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showLogoutDialog = false }) {
                    Text(stringResource(R.string.common_cancel))
                }
            }
        )
    }
}

@Composable
private fun SettingsRow(label: String, value: String) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp),
        horizontalArrangement = Arrangement.SpaceBetween
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurface
        )
    }
}
