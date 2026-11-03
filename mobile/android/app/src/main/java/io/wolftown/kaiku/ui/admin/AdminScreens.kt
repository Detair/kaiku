package io.wolftown.kaiku.ui.admin

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreenSizes
import io.wolftown.kaiku.ui.shared.KeySecondaryButton
import io.wolftown.kaiku.ui.shared.KeyTitle
import io.wolftown.kaiku.ui.shared.currentFragmentActivity

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AdminScaffold(title: String, onBack: () -> Unit, content: @Composable () -> Unit) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.common_back))
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp)) {
            content()
        }
    }
}

/** "Admin": a device step-up first, then the users with search and "New account". */
@Composable
fun AdminUsersScreen(onBack: () -> Unit, onOpenUser: (String) -> Unit, viewModel: AdminUsersViewModel = hiltViewModel()) {
    val ui by viewModel.ui.collectAsState()
    val activity = currentFragmentActivity()
    var newUsername by remember { mutableStateOf("") }
    var newDisplayName by remember { mutableStateOf("") }
    var creating by remember { mutableStateOf(false) }

    LaunchedEffect(ui.created) { ui.created?.let { viewModel.clearCreated(); onOpenUser(it.id) } }

    AdminScaffold(stringResource(R.string.admin_title), onBack) {
        ui.message?.let { KeyNotice(stringResource(it), warning = true) }
        if (!ui.elevated) {
            KeyBody(stringResource(R.string.admin_enter_explain))
            KeyPrimaryButton(stringResource(R.string.admin_enter), onClick = { activity?.let(viewModel::enter) }, working = ui.working)
            return@AdminScaffold
        }
        OutlinedTextField(
            value = ui.search, onValueChange = viewModel::search,
            label = { Text(stringResource(R.string.admin_search)) }, singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
        )
        ui.users.forEach { u ->
            Row(
                Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.otherAction).clickable { onOpenUser(u.id) }.padding(vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(u.displayName.ifEmpty { u.username }, fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
                    Text(u.username, fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            HorizontalDivider()
        }
        if (!creating) {
            KeySecondaryButton(stringResource(R.string.admin_new_account), onClick = { creating = true })
        } else {
            KeyTitle(stringResource(R.string.admin_new_account))
            KeyBody(stringResource(R.string.admin_new_account_explain), secondary = true)
            OutlinedTextField(newUsername, { newUsername = it }, label = { Text(stringResource(R.string.common_username)) },
                singleLine = true, modifier = Modifier.fillMaxWidth())
            OutlinedTextField(newDisplayName, { newDisplayName = it }, label = { Text(stringResource(R.string.admin_display_name)) },
                singleLine = true, modifier = Modifier.fillMaxWidth())
            KeyPrimaryButton(stringResource(R.string.admin_create), enabled = newUsername.isNotBlank(), working = ui.working,
                onClick = { viewModel.createUser(newUsername, newDisplayName, null) })
        }
    }
}

/** One account: devices ("Remove device now"), `may_approve`, open request, "Set up a phone". */
@Composable
fun AdminUserScreen(onBack: () -> Unit, onSetUpPhone: (String) -> Unit, viewModel: AdminUserViewModel = hiltViewModel()) {
    val ui by viewModel.ui.collectAsState()
    val activity = currentFragmentActivity()
    LaunchedEffect(Unit) { viewModel.load() }
    val u = ui.user
    AdminScaffold(u?.displayName?.ifEmpty { u.username } ?: stringResource(R.string.admin_title), onBack) {
        ui.message?.let { KeyNotice(stringResource(it), warning = true) }
        if (u == null) return@AdminScaffold
        Text(stringResource(R.string.security_key_devices), style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onBackground, modifier = Modifier.padding(top = 16.dp))
        (u.keyDevices + u.passkeys).forEach { d ->
            Row(Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.otherAction), verticalAlignment = Alignment.CenterVertically) {
                Text(d.label, fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground, modifier = Modifier.weight(1f))
                if (ui.canRemoveDevices) {
                    TextButton(onClick = { activity?.let { viewModel.removeNow(it, d) } }, enabled = !ui.working) {
                        Text(stringResource(R.string.admin_remove_now), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
                    }
                }
            }
        }
        Row(Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.otherAction).padding(top = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.admin_may_approve), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
                Text(stringResource(R.string.admin_may_approve_explain), fontSize = 16.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Switch(checked = u.mayApprove, onCheckedChange = { v -> activity?.let { viewModel.setMayApprove(it, v) } }, enabled = !ui.working)
        }
        u.openRequest?.let { KeyNotice(stringResource(R.string.admin_request_open), warning = true) }
        if (ui.canSetUpPhone) {
            KeyPrimaryButton(stringResource(R.string.admin_set_up_phone), onClick = { onSetUpPhone(u.id) }, enabled = u.openRequest == null)
        }
    }
}
