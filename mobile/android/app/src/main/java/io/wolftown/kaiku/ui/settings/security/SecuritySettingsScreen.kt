package io.wolftown.kaiku.ui.settings.security

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.domain.model.FactorState
import io.wolftown.kaiku.domain.model.KeyDeviceStatus
import io.wolftown.kaiku.ui.auth.setup.parseInstant
import io.wolftown.kaiku.ui.auth.setup.readyText
import io.wolftown.kaiku.ui.settings.PasskeysSection
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyScreenSizes
import io.wolftown.kaiku.ui.shared.currentFragmentActivity
import kotlinx.coroutines.launch

/**
 * Security settings (Task B11): this phone, your phones, passkeys (LP's
 * [PasskeysSection]), sessions, recovery codes, recovery contact, password and
 * TOTP read-only, and the security log. In a probation session everything
 * L2 owns is read-only; LP's section keeps its own rules.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SecuritySettingsScreen(
    onBack: () -> Unit,
    onAddPhone: () -> Unit,
    onSignedOut: () -> Unit,
    onOpenSecurityLog: () -> Unit,
    viewModel: SecuritySettingsViewModel = hiltViewModel(),
) {
    val ui by viewModel.ui.collectAsState()
    val activity = currentFragmentActivity()
    val scope = rememberCoroutineScope()
    var picking by remember { mutableStateOf<List<ContactCandidate>?>(null) }
    var askRemovePhone by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) { viewModel.load() }
    LaunchedEffect(ui.signedOut) { if (ui.signedOut) onSignedOut() }

    ui.newRecoveryCodes?.let { codes ->
        RecoveryCodesShownOnce(codes = codes, onDone = viewModel::acknowledgeRecoveryCodes)
        return
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.security_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.common_back))
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
        ) {
            if (ui.readOnly && !ui.loading) KeyNotice(stringResource(R.string.security_probation_read_only), warning = false)
            ui.message?.let { KeyNotice(stringResource(it), warning = true) }

            Section(stringResource(R.string.security_this_phone))
            Body(stringResource(if (ui.isKeyDevice) R.string.security_this_phone_key else R.string.security_this_phone_not_key))
            ui.thisPhoneUv?.let {
                Body(stringResource(if (it == UvClass.PER_USE) R.string.security_uv_biometric else R.string.security_uv_screen_lock), secondary = true)
            }
            Action(stringResource(R.string.security_sign_out), onClick = viewModel::signOut)
            if (ui.isKeyDevice) {
                Action(stringResource(R.string.security_sign_out_remove_phone), enabled = !ui.working, onClick = { askRemovePhone = true })
            }

            Section(stringResource(R.string.security_key_devices))
            ui.keyDevices.forEach { d ->
                Row(Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.otherAction), verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Body(d.label)
                        when {
                            d.isCurrent -> Body(stringResource(R.string.security_key_device_this), secondary = true)
                            d.state == KeyDeviceStatus.PENDING -> parseInstant(d.readyAt)?.let {
                                Body(stringResource(R.string.security_key_device_pending, readyText(it)), secondary = true)
                            }
                            else -> Unit
                        }
                    }
                    if (!d.isCurrent && !ui.readOnly) {
                        TextButton(onClick = { activity?.let { viewModel.removeKeyDevice(it, d) } }, enabled = !ui.working) {
                            Text(stringResource(R.string.security_remove), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
                        }
                    }
                }
            }
            if (ui.canAddPhone && !ui.readOnly) Action(stringResource(R.string.security_add_phone), onClick = onAddPhone)

            // LP's section, unchanged (C29); its Add reaches the key's step-up through
            // the @DeviceStepUp binding (DC1).
            HorizontalDivider(Modifier.padding(top = 12.dp))
            PasskeysSection()

            Section(stringResource(R.string.security_sessions))
            SessionsList(
                sessions = ui.sessions,
                readOnly = ui.readOnly || ui.working,
                onEnd = { s -> activity?.let { viewModel.endSession(it, s) } },
            )
            if (!ui.readOnly) {
                Action(stringResource(R.string.security_sessions_end_others), enabled = !ui.working, onClick = { activity?.let(viewModel::endOtherSessions) })
            }

            Section(stringResource(R.string.security_recovery_codes))
            RecoveryCodesCard(
                left = ui.me?.recoveryCodesLeft ?: 0,
                readOnly = ui.readOnly || ui.working,
                onCreate = { activity?.let(viewModel::createRecoveryCodes) },
            )

            Section(stringResource(R.string.security_contact))
            RecoveryContactCard(
                contact = ui.me?.recoveryContact,
                readOnly = ui.readOnly || ui.working,
                onChoose = { scope.launch { picking = viewModel.contactCandidates() } },
                onRemove = { activity?.let(viewModel::removeRecoveryContact) },
            )

            Section(stringResource(R.string.security_factors_title))
            Body(stringResource(R.string.security_factors_elsewhere), secondary = true)
            Body(stringResource(R.string.security_password_state, factorText(ui.me?.password)))
            Body(stringResource(R.string.security_totp_state, factorText(ui.me?.totp)))

            HorizontalDivider(Modifier.padding(vertical = 12.dp))
            Action(stringResource(R.string.security_log_open), onClick = onOpenSecurityLog)
            Spacer(Modifier.heightIn(min = 24.dp))
        }
    }

    picking?.let { list ->
        AlertDialog(
            onDismissRequest = { picking = null },
            title = { Text(stringResource(R.string.security_contact_pick_title)) },
            text = {
                Column {
                    if (list.isEmpty()) Text(stringResource(R.string.security_contact_pick_empty))
                    list.forEach { c ->
                        TextButton(
                            onClick = {
                                picking = null
                                activity?.let { viewModel.setRecoveryContact(it, c) }
                            },
                            modifier = Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.otherAction),
                        ) { Text(c.displayName.ifEmpty { c.username }, fontSize = 18.sp, color = MaterialTheme.colorScheme.onSurface) }
                    }
                }
            },
            confirmButton = {},
            dismissButton = { TextButton(onClick = { picking = null }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }

    if (ui.offerSignOutOthers) {
        AlertDialog(
            onDismissRequest = viewModel::dismissSignOutOthers,
            title = { Text(stringResource(R.string.security_sign_out_others_title)) },
            text = { Text(stringResource(R.string.security_sign_out_others_body)) },
            confirmButton = {
                TextButton(onClick = { activity?.let(viewModel::confirmSignOutOthers) }) {
                    Text(stringResource(R.string.security_sessions_end_others))
                }
            },
            dismissButton = { TextButton(onClick = viewModel::dismissSignOutOthers) { Text(stringResource(R.string.common_cancel)) } },
        )
    }

    if (askRemovePhone) {
        AlertDialog(
            onDismissRequest = { askRemovePhone = false },
            title = { Text(stringResource(R.string.security_sign_out_remove_phone)) },
            text = { Text(stringResource(R.string.security_remove_phone_explain)) },
            confirmButton = {
                TextButton(onClick = {
                    askRemovePhone = false
                    activity?.let(viewModel::signOutAndRemovePhone)
                }) { Text(stringResource(R.string.security_sign_out_remove_phone)) }
            },
            dismissButton = { TextButton(onClick = { askRemovePhone = false }) { Text(stringResource(R.string.common_cancel)) } },
        )
    }
}

@Composable
private fun factorText(f: FactorState?): String = when (f?.state) {
    null -> stringResource(R.string.security_factor_none)
    "active" -> stringResource(R.string.security_factor_active)
    else -> stringResource(R.string.security_factor_disabled)
}

@Composable
internal fun Section(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleMedium,
        color = MaterialTheme.colorScheme.onBackground,
        modifier = Modifier.padding(top = 20.dp, bottom = 8.dp),
    )
}

@Composable
internal fun Body(text: String, secondary: Boolean = false) {
    Text(
        text = text,
        fontSize = 18.sp,
        color = if (secondary) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onBackground,
    )
}

@Composable
internal fun Action(text: String, enabled: Boolean = true, onClick: () -> Unit) {
    OutlinedButton(
        onClick = onClick,
        enabled = enabled,
        modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp).heightIn(min = KeyScreenSizes.otherAction),
    ) {
        Text(text, fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
    }
    Spacer(Modifier.width(0.dp))
}
