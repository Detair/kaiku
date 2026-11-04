package io.wolftown.kaiku.ui.admin

import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.ui.approval.AddPhoneScreen
import io.wolftown.kaiku.ui.approval.IssuerMode
import io.wolftown.kaiku.ui.shared.KeyBody
import io.wolftown.kaiku.ui.shared.KeyNotice
import io.wolftown.kaiku.ui.shared.KeyPrimaryButton
import io.wolftown.kaiku.ui.shared.KeyScreen
import io.wolftown.kaiku.ui.shared.KeyScreenSizes
import io.wolftown.kaiku.ui.shared.KeyTitle

/**
 * F5c "Set up a phone" for someone else, from the admin's own key phone. For
 * an existing account it first points to the same-day path with the old phone
 * (6a); "Recover" names lost devices; the claimant check and its note are
 * required; each code is its own step-up.
 */
@Composable
fun AdminSetupPhoneScreen(onDone: () -> Unit, viewModel: AdminUserViewModel = hiltViewModel()) {
    val ui by viewModel.ui.collectAsState()
    LaunchedEffect(Unit) { if (viewModel.ui.value.user == null) viewModel.load() }
    val u = ui.user ?: return
    // Saved across rotation: the issuing flow and what the admin recorded stay what they were,
    // so a recreated screen resumes the same setup code instead of a blank form (Argon part4 #2).
    var recover by rememberSaveable { mutableStateOf(false) }
    var lostIds by rememberSaveable { mutableStateOf("") }
    val lost = lostIds.split(',').filter { it.isNotEmpty() }.toSet()
    var method by rememberSaveable { mutableStateOf(ClaimantCheck.IN_PERSON) }
    var note by rememberSaveable { mutableStateOf("") }
    var issuing by rememberSaveable { mutableStateOf(false) }

    if (issuing) {
        val mode = IssuerMode.Admin(
            userId = u.id,
            displayName = u.displayName.ifEmpty { u.username },
            mode = if (recover) "recovery" else "setup",
            claimantMethod = method,
            note = note.trim(),
            lostDeviceIds = if (recover) lost.toList() else emptyList(),
            newAccount = u.isNewAccount,
        )
        AddPhoneScreen(mode = mode, onDone = onDone, keypadHint = R.string.admin_type_code_hint)
        return
    }
    KeyScreen {
        KeyTitle(stringResource(R.string.admin_set_up_phone_for, u.displayName.ifEmpty { u.username }))
        if (!u.isNewAccount) KeyNotice(stringResource(R.string.admin_old_phone_hint), warning = false)
        if (u.keyDevices.isNotEmpty()) {
            Row(Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.secondaryButton), verticalAlignment = Alignment.CenterVertically) {
                Checkbox(checked = recover, onCheckedChange = { recover = it })
                Text(stringResource(R.string.admin_mode_recover), fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
            }
            if (recover) {
                KeyBody(stringResource(R.string.admin_lost_devices), secondary = true)
                u.keyDevices.forEach { d ->
                    Row(Modifier.fillMaxWidth().heightIn(min = KeyScreenSizes.secondaryButton), verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(checked = d.id in lost, onCheckedChange = { c -> lostIds = (if (c) lost + d.id else lost - d.id).joinToString(",") })
                        Text(d.label, fontSize = 18.sp, color = MaterialTheme.colorScheme.onBackground)
                    }
                }
            }
        }
        ClaimantCheckStep(method = method, onMethod = { method = it }, note = note, onNote = { note = it })
        KeyPrimaryButton(
            stringResource(R.string.admin_issue_code),
            enabled = ClaimantCheck.noteValid(note),
            onClick = { issuing = true },
        )
    }
}
